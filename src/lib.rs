//! This library provides the `YieldProgress` type, which allows a long-running async task
//! to report its progress, while also yielding to the scheduler (e.g. for the
//! single-threaded web/Wasm environment) and introducing cancellation points.
//!
//! These things go together because the rate at which it makes sense to yield (to avoid
//! event loop hangs) is similar to the rate at which it makes sense to report progress.
//!
//! `YieldProgress` is executor-independent; when it is constructed, the caller provides a
//! function for yielding.
//!
//! # Crate feature flags
//!
//! * `sync` (default): Adds `YieldProgress::split_evenly_concurrent()`.
//!
//!   Requires `std` to be available for the compilation target.
//!
//! * `log_hiccups`: Log intervals between yields longer than 100 ms, via the [`log`] library.
//!
//!   Requires `std` to be available for the compilation target.
//!   This might be removed in favor of something more configurable in future versions,
//!   in which case the feature flag may still exist but do nothing.
//!
//! [`log`]: https://docs.rs/log/0.4/

#![no_std]
#![deny(elided_lifetimes_in_paths)]
#![forbid(unsafe_code)]
#![warn(clippy::cast_lossless)]
#![warn(clippy::exhaustive_enums)]
#![warn(clippy::exhaustive_structs)]
#![warn(clippy::missing_panics_doc)]
#![warn(clippy::return_self_not_must_use)]
#![warn(clippy::wrong_self_convention)]
#![warn(missing_docs)]
#![warn(unused_lifetimes)]
#![warn(unused_qualifications)]

extern crate alloc;

#[cfg(any(feature = "sync", feature = "log_hiccups"))]
extern crate std;

use core::fmt;
use core::future::Future;
use core::iter::FusedIterator;
use core::panic::Location;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::string::ToString as _;
use alloc::sync::Arc;

#[cfg(doc)]
use core::task::Poll;

#[cfg(feature = "log_hiccups")]
use web_time::Instant;

mod basic_yield;
pub use basic_yield::basic_yield_now;

mod builder;
pub use builder::Builder;

#[cfg(feature = "sync")]
mod concurrent;

mod info;
pub use info::{ProgressInfo, YieldInfo};

/// We could import this alias from `futures-core` but that would be another non-dev dependency.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

type ProgressFn = dyn for<'a> Fn(&'a ProgressInfo<'a>) + Send + Sync + 'static;

type YieldFn = dyn for<'a> Fn(&'a YieldInfo<'a>) -> BoxFuture<'static, ()> + Send + Sync;

type Label = Arc<str>;

/// Allows a long-running async task to report its progress, while also yielding to the
/// scheduler (e.g. for single-threaded web environment) and introducing cancellation
/// points.
///
/// These things go together because the rate at which it makes sense to yield (to avoid event
/// loop hangs) is similar to the rate at which it makes sense to report progress.
///
/// ---
///
/// To construct a [`YieldProgress`], use the [`Builder`], or [`noop()`](YieldProgress::noop).
pub struct YieldProgress {
    start: f32,
    end: f32,

    /// Name given to this specific portion of work. Inherited from the parent if not
    /// overridden.
    ///
    /// TODO: Eventually we will want to have things like "label this segment as a
    /// fallback if it has no better label", which will require some notion of distinguishing
    /// inheritance from having been explicitly set.
    label: Option<Label>,

    yielding: BoxYielding,
    // TODO: change progress reporting interface to support efficient handling of
    // the label string being the same as last time.
    progressor: Arc<ProgressFn>,
}

/// Piggyback on the `Arc` we need to store the `dyn Fn` anyway to also store some state.
struct Yielding<F: ?Sized> {
    #[cfg(feature = "log_hiccups")]
    state: std::sync::Mutex<YieldState>,

    yielder: F,
}

type BoxYielding = Arc<Yielding<YieldFn>>;

/// Information about the last yield performed.
/// Compared with the current state when the `log_hiccups` feature is enabled.
#[cfg(feature = "log_hiccups")]
#[derive(Clone)]
struct YieldState {
    /// The most recent instant at which `yielder`'s future completed.
    /// Used to detect overlong time periods between yields.
    last_finished_yielding: Instant,

    last_yield_location: &'static Location<'static>,

    last_yield_label: Option<Label>,
}

impl fmt::Debug for YieldProgress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("YieldProgress")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl YieldProgress {
    /// Returns a [`YieldProgress`] that does no progress reporting **and no yielding at all**.
    ///
    /// This may be used, for example, to call a function that accepts [`YieldProgress`] and
    /// is not `async` for any other reason.
    /// It should not be used merely because no progress reporting is desired; in that case
    /// use [`Builder`] instead so that a yield function can be provided.
    ///
    /// # Example
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// # use yield_progress::YieldProgress;
    /// let mut progress = YieldProgress::noop();
    /// // These calls will have no effect.
    /// progress.set_label("a tree falls in a forest");
    /// progress.progress(0.12345).await;
    /// # }
    /// ```
    pub fn noop() -> Self {
        Builder::new()
            .yield_using(|_| core::future::ready(()))
            .build()
    }

    /// Add a name for the portion of work this [`YieldProgress`] covers, which will be
    /// used by all future progress updates.
    ///
    /// If there is already a label, it will be overwritten.
    ///
    /// This does not immediately report progress; that is, the label will not be visible
    /// anywhere until the next operation that does. Future versions may report it immediately.
    ///
    /// # Example
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// # use yield_progress::YieldProgress;
    /// async fn process_things(progress: YieldProgress, things: &[String]) {
    ///     let len = things.len();
    ///     for ((mut progress, thing), i) in progress.split_evenly(len).zip(things).zip(1..) {
    ///         progress.set_label(format_args!("Processing {i}/{len}: {thing}"));
    ///         progress.progress(0.0).await;
    ///         // ... Do actual work here ...
    ///         progress.finish().await;
    ///     }
    /// }
    /// # let expected_label = &*Box::leak(Box::new(std::sync::OnceLock::<String>::new()));
    /// # process_things(
    /// #     yield_progress::Builder::new()
    /// #         .progress_using(move |info| {
    /// #             if !info.label_str().is_empty() {
    /// #                 expected_label.set(info.label_str().to_owned());
    /// #             }
    /// #         })
    /// #         .build(),
    /// #     &[String::from("hello world")],
    /// # ).await;
    /// # assert_eq!(
    /// #     expected_label.get().map(|s| &**s),
    /// #     Some::<&str>("Processing 1/1: hello world"),
    /// # );
    /// # }
    /// ```
    pub fn set_label(&mut self, label: impl fmt::Display) {
        self.set_label_internal(Some(Arc::from(label.to_string())))
    }

    fn set_label_internal(&mut self, label: Option<Label>) {
        self.label = label;
    }

    /// Map a `0..=1` value to `self.start..=self.end`.
    #[track_caller]
    fn point_in_range(&self, mut x: f32) -> f32 {
        x = x.clamp(0.0, 1.0);
        if !x.is_finite() {
            if cfg!(debug_assertions) {
                panic!("NaN progress value");
            } else {
                x = 0.5;
            }
        }
        self.start + (x * (self.end - self.start))
    }

    /// Report the current amount of progress (a number from 0 to 1) and yield.
    ///
    /// The value *may* be less than previously given values.
    ///
    /// # Example
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// # use yield_progress::YieldProgress;
    /// # pub fn first_half_of_work() {}
    /// # pub fn second_half_of_work() {}
    /// async fn do_work(progress: YieldProgress) {
    ///     first_half_of_work();
    ///     progress.progress(0.5).await;
    ///     second_half_of_work();
    ///     progress.finish().await;
    /// }
    /// # do_work(YieldProgress::noop()).await;
    /// # }
    /// ```
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn progress(&self, progress_fraction: f32) -> impl Future<Output = ()> + use<> {
        let location = Location::caller();

        self.progress_without_yield(progress_fraction);

        self.yielding.clone().yield_only(
            location,
            #[cfg(feature = "log_hiccups")]
            self.label.clone(),
        )
    }

    /// Report the current amount of progress (a number from 0 to 1) without yielding.
    ///
    /// Caution: Not yielding may mean that the display of progress to the user does not
    /// update. This should be used only when necessary for non-async code.
    #[track_caller]
    pub fn progress_without_yield(&self, progress_fraction: f32) {
        let location = Location::caller();
        self.send_progress(progress_fraction, self.label.as_ref(), location);
    }

    /// Yield only; that is, call the yield function contained within this [`YieldProgress`].
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn yield_without_progress(&self) -> impl Future<Output = ()> + Send + use<> {
        let location = Location::caller();

        self.yielding.clone().yield_only(
            location,
            #[cfg(feature = "log_hiccups")]
            self.label.clone(),
        )
    }

    /// Assemble a [`ProgressInfo`] using self's range and send it to the progress function.
    /// This differs from `progress_without_yield()` by taking an explicit label and location;
    /// only the range and destination from `self` is used.
    fn send_progress(
        &self,
        progress_fraction: f32,
        label: Option<&Label>,
        location: &Location<'_>,
    ) {
        (self.progressor)(&ProgressInfo {
            fraction: self.point_in_range(progress_fraction),
            label,
            location,
        });
    }

    /// Report that 100% of progress has been made.
    ///
    /// This is identical to `.progress(1.0)` but consumes the `YieldProgress` object.
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn finish(self) -> impl Future<Output = ()> + Send + use<> {
        self.progress(1.0)
    }

    /// Report that the given amount of progress has been made, then return
    /// a [`YieldProgress`] covering the remaining range.
    ///
    /// # Example
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// # use yield_progress::YieldProgress;
    /// # pub fn first_half_of_work() {}
    /// # pub async fn second_half_of_work(progress: YieldProgress) {
    /// #     progress.finish().await;
    /// # }
    /// async fn do_work(progress: YieldProgress) {
    ///     first_half_of_work();
    ///     second_half_of_work(progress.finish_and_cut(0.5).await).await;
    /// }
    /// # do_work(YieldProgress::noop()).await;
    /// # }
    /// ```
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn finish_and_cut(
        self,
        progress_fraction: f32,
    ) -> impl Future<Output = Self> + Send + use<> {
        // Efficiency note: this is structured so that `a` can be dropped immediately
        // and does not live on in the future.
        let [a, b] = self.split(progress_fraction);
        a.progress_without_yield(1.0);
        async move {
            b.yield_without_progress().await;
            b
        }
    }

    /// Report the _beginning_ of a unit of work of size `progress_fraction` and described
    /// by `label`. That fraction is cut off of the beginning range of `self`, and returned
    /// as a separate [`YieldProgress`].
    ///
    /// # Example
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// # use yield_progress::YieldProgress;
    /// # pub async fn first_half_of_work(progress: YieldProgress) {
    /// #     progress.finish().await;
    /// # }
    /// # pub fn second_half_of_work() {}
    /// async fn do_work(mut progress: YieldProgress) {
    ///     first_half_of_work(progress.start_and_cut(0.5, "first half").await).await;
    ///     second_half_of_work();
    ///     progress.finish().await;
    /// }
    /// # do_work(YieldProgress::noop()).await;
    /// # }
    /// ```
    #[track_caller]
    pub fn start_and_cut<L: fmt::Display>(
        &mut self,
        cut: f32,
        label: L,
    ) -> impl Future<Output = Self> + Send + use<L> {
        // Note: the use<L> bound is not required and is in fact unduly restrictive.
        // However, removing it is not currently suppoerted by rustc.

        let cut_abs = self.point_in_range(cut);
        let mut portion = self.with_new_range(self.start, cut_abs);
        self.start = cut_abs;

        portion.set_label(label);
        async {
            portion.progress(0.0).await;
            portion
        }
    }

    fn with_new_range(&self, start: f32, end: f32) -> Self {
        Self {
            start,
            end,
            label: self.label.clone(),
            yielding: Arc::clone(&self.yielding),
            progressor: Arc::clone(&self.progressor),
        }
    }

    /// Construct two new [`YieldProgress`] which divide the progress value into two
    /// subranges.
    ///
    /// The returned instances should be used in sequence, but this is not enforced.
    /// Using them concurrently will result in the progress bar jumping backwards.
    ///
    /// # Example
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// # use yield_progress::YieldProgress;
    /// # pub async fn first_half_of_work(progress: YieldProgress) {
    /// #     progress.finish().await;
    /// # }
    /// # pub async fn second_half_of_work(progress: YieldProgress) {
    /// #     progress.finish().await;
    /// # }
    /// async fn do_work(mut progress: YieldProgress) {
    ///     let [p1, p2] = progress.split(0.5);
    ///     first_half_of_work(p1).await;
    ///     second_half_of_work(p2).await;
    /// }
    /// # do_work(YieldProgress::noop()).await;
    /// # }
    /// ```
    pub fn split(self, cut: f32) -> [Self; 2] {
        let cut_abs = self.point_in_range(cut);
        [
            self.with_new_range(self.start, cut_abs),
            self.with_new_range(cut_abs, self.end),
        ]
    }

    /// Construct many new [`YieldProgress`] which together divide the progress value into
    /// `count` subranges.
    ///
    /// The returned instances should be used in sequence, but this is not enforced.
    /// Using them concurrently will result in the progress bar jumping backwards.
    ///
    /// # Example
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// # use yield_progress::YieldProgress;
    /// # struct Thing;
    /// # fn process_one_thing(thing: Thing) {}
    /// async fn process_things(progress: YieldProgress, things: Vec<Thing>) {
    ///     for (mut progress, thing) in progress.split_evenly(things.len()).zip(things) {
    ///         process_one_thing(thing);
    ///         progress.finish().await;
    ///     }
    /// }
    /// # process_things(YieldProgress::noop(), vec![Thing]).await;
    /// # }
    /// ```
    pub fn split_evenly(
        self,
        count: usize,
    ) -> impl DoubleEndedIterator<Item = YieldProgress> + ExactSizeIterator + FusedIterator + use<>
    {
        (0..count).map(move |index| {
            self.with_new_range(
                self.point_in_range(index as f32 / count as f32),
                self.point_in_range((index as f32 + 1.0) / count as f32),
            )
        })
    }

    /// Construct many new [`YieldProgress`] which will collectively advance `self` to completion
    /// when they have all been advanced to completion, and which may be used concurrently.
    ///
    /// This is identical in effect to [`YieldProgress::split_evenly()`], except that it comprehends
    /// concurrent operations — the progress of `self` is the sum of the progress of the subtasks.
    /// To support this, it must allocate storage for the state tracking and synchronization, and
    /// every progress update must calculate the sum from all subtasks. Therefore, for efficiency,
    /// do not use this except when concurrency is actually present.
    ///
    /// The label passed through will be the label from the first subtask that has a progress
    /// value less than 1.0. This choice may be changed in the future if the label system is
    /// elaborated.
    ///
    /// # Example
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// use yield_progress::YieldProgress;
    /// use tokio::task::JoinSet;
    /// # struct Thing;
    /// # async fn process_one_thing(progress: YieldProgress, thing: Thing) {
    /// #     progress.finish().await;
    /// # }
    ///
    /// async fn process_things(progress: YieldProgress, things: Vec<Thing>) {
    ///     let mut join_set = tokio::task::JoinSet::new();
    ///     for (mut progress, thing) in progress.split_evenly_concurrent(things.len()).zip(things) {
    ///         join_set.spawn(process_one_thing(progress, thing));
    ///     }
    ///     join_set.join_all().await;
    /// }
    /// # process_things(YieldProgress::noop(), vec![Thing]).await;
    /// # }
    /// ```
    #[cfg(feature = "sync")]
    pub fn split_evenly_concurrent(
        self,
        count: usize,
    ) -> impl DoubleEndedIterator<Item = YieldProgress> + ExactSizeIterator + FusedIterator + use<>
    {
        let yielding = self.yielding.clone();
        let conc = concurrent::ConcurrentProgress::new(self, count);
        (0..count).map(move |index| {
            Builder::new()
                .yielding_internal(yielding.clone())
                .progress_using(Arc::clone(&conc).progressor(index))
                .build()
        })
    }
}

impl<F> Yielding<F>
where
    F: ?Sized + for<'a> Fn(&'a YieldInfo<'a>) -> BoxFuture<'static, ()> + Send + Sync,
{
    #[allow(clippy::manual_async_fn)] // false positive from cfg
    fn yield_only(
        self: Arc<Self>,
        location: &'static Location<'static>,
        #[cfg(feature = "log_hiccups")] mut label: Option<Label>,
    ) -> impl Future<Output = ()> + use<F> {
        #[cfg(feature = "log_hiccups")]
        {
            #[allow(unused)] // may be redundant depending on other features
            use alloc::format;
            use core::time::Duration;

            // Note that we avoid holding the lock while calling yielder().
            // The worst outcome of an inconsistency is that we will output a meaningless
            // "between {location} and {location}" message, but none should occur because
            // [`YieldProgress`] is intended to be used in a sequential manner.
            let previous_state: YieldState = { self.state.lock().unwrap().clone() };

            let delta = Instant::now().duration_since(previous_state.last_finished_yielding);
            if delta > Duration::from_millis(100) {
                let last_label = previous_state.last_yield_label;
                log::trace!(
                    "Yielding after {delta} ms between {old_location} and {new_location} {rel}",
                    delta = delta.as_millis(),
                    old_location = previous_state.last_yield_location,
                    new_location = location,
                    rel = if label == last_label {
                        format!("during {label:?}")
                    } else {
                        format!("between {last_label:?} and {label:?}")
                    }
                );
            }
        }

        // TODO: Since we're tracking time, we might as well decide whether to not bother
        // yielding if it has been a short time ... except that different yielders might
        // want different granularities/policies.

        // Efficiency: This explicit `async` block somehow improves the future data size,
        // compared to `async fn`, by not allocating both a local and a capture for all of
        // `self`, `location`, and `label`. Seems odd that this helps...
        async move {
            let yield_future = {
                // Efficiency: This block avoids holding the temp `YieldInfo` across the await.
                (self.yielder)(&YieldInfo { location })
            };
            yield_future.await;

            #[cfg(feature = "log_hiccups")]
            {
                let mut state = self.state.lock().unwrap();

                state.last_yield_location = location;
                // Efficiency: this `Option::take()` avoids generating a drop flag.
                state.last_yield_label = label.take();

                state.last_finished_yielding = Instant::now();
            }
        }
    }
}
