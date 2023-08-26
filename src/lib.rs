//! This library provides the `YieldProgress` type, which allows a long-running async task
//! to report its progress, while also yielding to the scheduler (e.g. for the
//! single-threaded web/Wasm environment) and introducing cancellation points.
//!
//! These things go together because the rate at which it makes sense to yield (to avoid
//! event loop hangs) is similar to the rate at which it makes sense to report progress.
//!
//! `YieldProgress` is executor-independent; when it is constructed, the caller provides a
//! function for yielding.

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

use core::fmt;
use core::future::Future;
use core::panic::Location;
use core::pin::Pin;
use core::time::Duration;

#[cfg(doc)]
use core::task::Poll;

// TODO: Make thread-safety optional so we can be alloc-but-not-std
use std::sync::{Arc, Mutex};

// TODO: Make time checks optional
use instant::Instant;

#[cfg(test)]
mod tests;

/// We could import this alias from `futures-core` but that would be another non-dev dependency.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

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
pub struct YieldProgress {
    start: f32,
    end: f32,

    /// Name given to this specific portion of work. Inherited from the parent if not
    /// overridden.
    ///
    /// TODO: Eventually we will want to have things like "label this segment as a
    /// fallback if it has no better label", which will require some notion of distinguishing
    /// inheritance from having been explicitly set.
    label: Option<Arc<str>>,

    yielding: Arc<Yielding<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>>,
    // TODO: change progress reporting interface to support efficient handling of
    // the label string being the same as last time.
    #[allow(clippy::type_complexity)]
    progressor: Arc<dyn Fn(f32, &str) + Send + Sync>,
}

/// Piggyback on the `Arc` we need to store the `dyn Fn` anyway to also store some state.
struct Yielding<F: ?Sized> {
    state: Mutex<YieldState>,

    yielder: F,
}

#[derive(Clone)]
struct YieldState {
    /// The most recent instant at which `yielder`'s future completed.
    /// Used to detect overlong time periods between yields.
    last_finished_yielding: Instant,
    last_yield_location: &'static Location<'static>,
    last_yield_label: Option<Arc<str>>,
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
    /// It will also report any excessively-long intervals between yields using the [`log`]
    /// library. “Excessively long” is currently defined as 100&nbsp;ms.
    /// The first interval starts  when function is called, as if this is the first yield.
    /// This may become more configurable in future versions.
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
    pub fn new<Y, YFut, P>(yielder: Y, progressor: P) -> Self
    where
        Y: Fn() -> YFut + Send + Sync + 'static,
        YFut: Future<Output = ()> + Send + 'static,
        P: Fn(f32, &str) + Send + Sync + 'static,
    {
        let yielding: Arc<Yielding<_>> = Arc::new(Yielding {
            state: Mutex::new(YieldState {
                last_finished_yielding: Instant::now(),
                last_yield_location: Location::caller(),
                last_yield_label: None,
            }),
            yielder: move || -> BoxFuture<'static, ()> { Box::pin(yielder()) },
        });

        Self {
            start: 0.0,
            end: 1.0,
            label: None,
            yielding,
            progressor: Arc::new(progressor),
        }
    }

    /// Returns a [`YieldProgress`] that does no progress reporting and no yielding.
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
        Self::new(|| std::future::ready(()), |_, _| {})
    }

    /// Add a name for the portion of work this [`YieldProgress`] covers, which will be
    /// used by all future progress updates.
    ///
    /// If there is already a label, it will be overwritten.
    ///
    /// This does not immediately report progress; that is, the label will not be visible
    /// anywhere until the next operation that does. Future versions may report it immediately.
    pub fn set_label(&mut self, label: impl fmt::Display) {
        self.label = Some(Arc::from(label.to_string()))
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
    pub fn progress(&self, progress_fraction: f32) -> impl Future<Output = ()> + Send + 'static {
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
        (self.progressor)(
            self.point_in_range(progress_fraction),
            self.label
                .as_ref()
                .map_or("", |arc_str_ref| -> &str { arc_str_ref }),
        );
    }

    /// Yield only; that is, call the yield function contained within this [`YieldProgress`].
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn yield_without_progress(&self) -> impl Future<Output = ()> + Send + 'static {
        let location = Location::caller();
        let label = self.label.clone();

        self.yielding.clone().yield_only(location, label)
    }

    /// Report that 100% of progress has been made.
    ///
    /// This is identical to `.progress(1.0)` but consumes the `YieldProgress` object.
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn finish(self) -> impl Future<Output = ()> + Send + 'static {
        self.progress(1.0)
    }

    /// Report that the given amount of progress has been made, then return
    /// a [`YieldProgress`] covering the remaining range.
    #[track_caller] // This is not an `async fn` because `track_caller` is not compatible
    pub fn finish_and_cut(
        self,
        progress_fraction: f32,
    ) -> impl Future<Output = Self> + Send + 'static {
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
    ) -> impl Future<Output = Self> + Send + 'static {
        let cut_abs = self.point_in_range(cut);
        let mut portion = self.with_new_range(0.0, cut_abs);
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
    pub fn split(self, cut: f32) -> [Self; 2] {
        let cut_abs = self.point_in_range(cut);
        [
            self.with_new_range(self.start, cut_abs),
            self.with_new_range(cut_abs, self.end),
        ]
    }

    /// Split into even subdivisions.
    pub fn split_evenly(
        self,
        count: usize,
    ) -> impl DoubleEndedIterator<Item = YieldProgress> + ExactSizeIterator + std::iter::FusedIterator
    {
        (0..count).map(move |index| {
            self.with_new_range(
                self.point_in_range(index as f32 / count as f32),
                self.point_in_range((index as f32 + 1.0) / count as f32),
            )
        })
    }
}

impl<F: ?Sized + Fn() -> BoxFuture<'static, ()> + Send + Sync> Yielding<F> {
    async fn yield_only(
        self: Arc<Self>,
        location: &'static Location<'static>,
        label: Option<Arc<str>>,
    ) {
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

        // TODO: Since we're tracking time, we might as well decide whether to not bother
        // yielding if it has been a short time ... except that different yielders might
        // want different granularities/policies.
        (self.yielder)().await;

        {
            let mut state = self.state.lock().unwrap();
            state.last_finished_yielding = Instant::now();
            state.last_yield_location = location;
            state.last_yield_label = label;
        }
    }
}
