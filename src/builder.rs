use core::future::Future;
use core::panic::Location;

use alloc::boxed::Box;
use alloc::sync::Arc;

use std::sync::Mutex;

use instant::Instant;

use crate::{basic_yield_now, BoxFuture, YieldProgress, YieldState, Yielding};

/// Builder for creating root [`YieldProgress`] instances.
#[derive(Clone)]
#[must_use]
pub struct Builder {
    yielding: Arc<Yielding<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>>,
    progressor: Arc<crate::ProgressFn>,
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
                yielder: move || -> BoxFuture<'static, ()> { Box::pin(basic_yield_now()) },
                state: Mutex::new(YieldState {
                    last_finished_yielding: Instant::now(),
                    last_yield_location: Location::caller(),
                    last_yield_label: None,
                }),
            }),
            progressor: Arc::new(|_, _| {}),
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
        Y: Fn() -> YFut + Send + Sync + 'static,
        YFut: Future<Output = ()> + Send + 'static,
    {
        let new_yielding = Arc::new(Yielding {
            yielder: move || -> BoxFuture<'static, ()> { Box::pin(function()) },
            state: Mutex::new(self.yielding.state.lock().unwrap().clone()),
        });
        self.yielding = new_yielding;
        self
    }

    /// Set the function which will be called in order to report progress.
    ///
    /// If this is called more than once, the previous function will be discarded (there is no list
    /// of callbacks). If this is not called, the default function used does nothing.
    pub fn progress_using<P>(mut self, function: P) -> Self
    where
        P: Fn(f32, &str) + Send + Sync + 'static,
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
