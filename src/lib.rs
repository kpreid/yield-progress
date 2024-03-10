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
//! * `sync` (default): Implements `YieldProgress: Send + Sync` for use with multi-threaded executors.
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

#[cfg(any(test, feature = "sync"))]
#[cfg_attr(test, macro_use)]
extern crate std;

use core::fmt;
use core::future::Future;
use core::iter::FusedIterator;
use core::panic::Location;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::string::ToString as _;

#[cfg(doc)]
use core::task::Poll;

#[cfg(feature = "log_hiccups")]
use web_time::Instant;

mod basic_yield;
pub use basic_yield::basic_yield_now;

mod builder;
pub use builder::Builder;

mod concurrent;
use concurrent::ConcurrentProgress;

mod maybe_sync;
use maybe_sync::*;

mod info;
pub use info::{ProgressInfo, YieldInfo};

/// We could import this alias from `futures-core` but that would be another non-dev dependency.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// We allow !Sync progress functions but only internally; this type doesn't control the API.
#[cfg(feature = "sync")]
type ProgressFn = dyn for<'a> Fn(&'a ProgressInfo<'a>) + Send + Sync + 'static;
#[cfg(not(feature = "sync"))]
type ProgressFn = dyn for<'a> Fn(&'a ProgressInfo<'a>) + 'static;

type YieldFn = dyn for<'a> Fn(&'a YieldInfo<'a>) -> BoxFuture<'static, ()> + Send + Sync;

/// Allows a long-running async task to report its progress, while also yielding to the
/// scheduler (e.g. for single-threaded web environment) and introducing cancellation
/// points.
///
/// These things go together because the rate at which it makes sense to yield (to avoid event
/// loop hangs) is similar to the rate at which it makes sense to report progress.
///
/// Note that while a [`YieldProgress`] is [`Send`] and [`Sync`] in order to be used within tasks
/// that may be moved between threads, it does not currently support meaningfully being used from
/// multiple threads or futures at once — only within a fully sequential operation. Future versions
/// may include a “parallel split” operation but the current one does not.
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
    label: Option<MaRc<str>>,

    yielding: BoxYielding,
    // TODO: change progress reporting interface to support efficient handling of
    // the label string being the same as last time.
    progressor: MaRc<ProgressFn>,
}

/// Piggyback on the `Arc` we need to store the `dyn Fn` anyway to also store some state.
struct Yielding<F: ?Sized> {
    state: StateCell<YieldState>,

    yielder: F,
}

type BoxYielding = MaRc<Yielding<YieldFn>>;

#[derive(Clone)]
struct YieldState {
    /// The most recent instant at which `yielder`'s future completed.
    /// Used to detect overlong time periods between yields.
    #[cfg(feature = "log_hiccups")]
    last_finished_yielding: Instant,

    last_yield_location: &'static Location<'static>,

    last_yield_label: Option<MaRc<str>>,
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
    /// Construct a new [`YieldProgress`], which will call `yielder` to yield and
    /// `progressor` to report progress.
    ///
    /// * `yielder` should return a `Future` that returns [`Poll::Pending`] at least once,
    ///   and may perform other executor-specific actions to assist with scheduling other tasks.
    /// * `progressor` is called with the progress fraction (a number between 0 and 1) and a
    ///   label for the current portion of work (which will be `""` if no label has been set).
    ///
    /// # Example
    ///
    /// ```
    /// use yield_progress::YieldProgress;
    /// # struct Pb;
    /// # impl Pb { fn set_value(&self, _value: f32) {} }
    /// # let some_progress_bar = Pb;
    /// // let some_progress_bar = ...;
    ///
    /// let progress = YieldProgress::new(
    ///     tokio::task::yield_now,
    ///     move |fraction, _label| {
    ///         some_progress_bar.set_value(fraction);
    ///     }
    /// );
    /// ```
    #[track_caller]
    #[deprecated = "use `yield_progress::Builder` instead"]
    pub fn new<Y, YFut, P>(yielder: Y, progressor: P) -> Self
    where
        Y: Fn() -> YFut + Send + Sync + 'static,
        YFut: Future<Output = ()> + Send + 'static,
        P: Fn(f32, &str) + Send + Sync + 'static,
    {
        Builder::new()
            .yield_using(move |_| yielder())
            .progress_using(move |info| progressor(info.fraction(), info.label_str()))
            .build()
    }

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
    /// use yield_progress::YieldProgress;
    ///
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
    pub fn set_label(&mut self, label: impl fmt::Display) {
        self.set_label_internal(Some(MaRc::from(label.to_string())))
    }

    fn set_label_internal(&mut self, label: Option<MaRc<str>>) {
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
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn progress(&self, progress_fraction: f32) -> maybe_send_impl_future!(()) {
        let location = Location::caller();
        let label = self.label.clone();

        self.progress_without_yield(progress_fraction);

        self.yielding.clone().yield_only(location, label)
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
    pub fn yield_without_progress(&self) -> maybe_send_impl_future!(()) {
        let location = Location::caller();
        let label = self.label.clone();

        self.yielding.clone().yield_only(location, label)
    }

    /// Assemble a [`ProgressInfo`] using self's range and send it to the progress function.
    /// This differs from `progress_without_yield()` by taking an explicit label and location;
    /// only the range and destination from `self` is used.
    fn send_progress(
        &self,
        progress_fraction: f32,
        label: Option<&MaRc<str>>,
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
    pub fn finish(self) -> maybe_send_impl_future!(()) {
        self.progress(1.0)
    }

    /// Report that the given amount of progress has been made, then return
    /// a [`YieldProgress`] covering the remaining range.
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn finish_and_cut(self, progress_fraction: f32) -> maybe_send_impl_future!(Self) {
        let [a, b] = self.split(progress_fraction);
        let progress_future = a.finish();
        async move {
            progress_future.await;
            b
        }
    }

    /// Report the _beginning_ of a unit of work of size `progress_fraction` and described
    /// by `label`. That fraction is cut off of the beginning range of `self`, and returned
    /// as a separate [`YieldProgress`].
    ///
    /// ```no_run
    /// # async fn foo() {
    /// # use yield_progress::YieldProgress;
    /// # let mut main_progress = YieldProgress::noop();
    /// let a_progress = main_progress.start_and_cut(0.5, "task A").await;
    /// // do task A...
    /// a_progress.finish().await;
    /// // continue using main_progress...
    /// # }
    /// ```
    #[track_caller]
    pub fn start_and_cut(
        &mut self,
        cut: f32,
        label: impl fmt::Display,
    ) -> maybe_send_impl_future!(Self) {
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
            yielding: MaRc::clone(&self.yielding),
            progressor: MaRc::clone(&self.progressor),
        }
    }

    /// Construct two new [`YieldProgress`] which divide the progress value into two
    /// subranges.
    ///
    /// The returned instances should be used in sequence, but this is not enforced.
    /// Using them concurrently will result in the progress bar jumping backwards.
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
    pub fn split_evenly(
        self,
        count: usize,
    ) -> impl DoubleEndedIterator<Item = YieldProgress> + ExactSizeIterator + FusedIterator {
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
    pub fn split_concurrent(
        self,
        count: usize,
    ) -> impl DoubleEndedIterator<Item = YieldProgress> + ExactSizeIterator + FusedIterator {
        let yielding = self.yielding.clone();
        let conc = ConcurrentProgress::new(self, count);
        (0..count).map(move |index| {
            let mut builder = Builder::new().yielding_internal(yielding.clone());
            // The progressor may be `!Sync` if our `sync` feature is disabled.
            // This is prohibited in our API, but it's safe for us as long as we match the feature.
            // Therefore, bypass the method and assign the field directly.
            builder.progressor = MaRc::new(MaRc::clone(&conc).progressor(index));
            builder.build()
        })
    }
}

impl<F: ?Sized + for<'a> Fn(&'a YieldInfo<'a>) -> BoxFuture<'static, ()> + Send + Sync>
    Yielding<F>
{
    async fn yield_only(
        self: MaRc<Self>,
        location: &'static Location<'static>,
        label: Option<MaRc<str>>,
    ) {
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
        (self.yielder)(&YieldInfo { location }).await;

        {
            let mut state = self.state.lock().unwrap();

            state.last_yield_location = location;
            state.last_yield_label = label;

            #[cfg(feature = "log_hiccups")]
            {
                state.last_finished_yielding = Instant::now();
            }
        }
    }
}
