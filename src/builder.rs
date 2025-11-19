use core::future::Future;

use alloc::boxed::Box;
use alloc::sync::Arc;

use crate::{BoxFuture, ProgressInfo, YieldInfo, YieldProgress, Yielding, basic_yield_now};

/// Builder for creating root [`YieldProgress`] instances.
///
/// # Example
///
/// ```
/// # struct Pb;
/// # impl Pb { fn set_value(&self, _value: f32) {} }
/// # let some_progress_bar = Pb;
/// let progress = yield_progress::Builder::new()
///     .yield_using(|_| tokio::task::yield_now())
///     .progress_using(move |info| {
///         some_progress_bar.set_value(info.fraction());
///     })
///     .build();
/// ```
#[derive(Clone)]
#[must_use]
pub struct Builder {
    yielding: Arc<Yielding<crate::YieldFn>>,
    /// public to allow overriding the API `Sync` requirement
    pub(crate) progressor: Arc<crate::ProgressFn>,
}

impl Builder {
    /// Constructs a [`Builder`].
    ///
    /// The default values are:
    ///
    /// * The yield function is [`basic_yield_now`].
    /// * The progress function does nothing.
    ///
    /// The call site of this function is considered to be the yield point preceding the first actual
    /// yield point.
    #[track_caller]
    pub fn new() -> Builder {
        Builder {
            yielding: Arc::new(Yielding {
                yielder: move |_info: &YieldInfo<'_>| -> BoxFuture<'static, ()> {
                    Box::pin(basic_yield_now())
                },
                #[cfg(feature = "log_hiccups")]
                state: std::sync::Mutex::new(crate::YieldState {
                    last_finished_yielding: web_time::Instant::now(),
                    last_yield_location: core::panic::Location::caller(),
                    last_yield_label: None,
                }),
            }),
            progressor: Arc::new(|_| {}),
        }
    }

    /// Construct a new [`YieldProgress`] using the behaviors specified by this builder.
    pub fn build(self) -> YieldProgress {
        let Self {
            yielding,
            progressor,
        } = self;

        YieldProgress {
            start: 0.0,
            end: 1.0,
            label: None,
            yielding,
            progressor,
        }
    }

    /// Set the function which will be called in order to yield.
    ///
    /// See [`basic_yield_now()`] for the default implementation used if this is not not set, and
    /// some information about what you might want to pass here instead.
    #[allow(clippy::missing_panics_doc)] // internal, can't happen
    pub fn yield_using<Y, YFut>(mut self, function: Y) -> Self
    where
        Y: for<'a> Fn(&'a YieldInfo<'a>) -> YFut + Send + Sync + 'static,
        YFut: Future<Output = ()> + Send + 'static,
    {
        let new_yielding = Arc::new(Yielding {
            yielder: move |info: &YieldInfo<'_>| -> BoxFuture<'static, ()> {
                Box::pin(function(info))
            },
            #[cfg(feature = "log_hiccups")]
            state: std::sync::Mutex::new(self.yielding.state.lock().unwrap().clone()),
        });
        self.yielding = new_yielding;
        self
    }

    /// Internal version of `yield_using` which takes an already boxed function and yielding state.
    #[cfg_attr(not(feature = "sync"), allow(dead_code))]
    pub(crate) fn yielding_internal(mut self, new_yielding: crate::BoxYielding) -> Self {
        self.yielding = new_yielding;
        self
    }

    /// Set the function which will be called in order to report progress.
    ///
    /// If this is called more than once, the previous function will be discarded (there is no list
    /// of callbacks). If this is not called, the default function used does nothing.
    pub fn progress_using<P>(mut self, function: P) -> Self
    where
        P: for<'a> Fn(&'a ProgressInfo<'a>) + Send + Sync + 'static,
    {
        self.progressor = Arc::new(function);
        self
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}
