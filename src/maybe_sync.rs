//! Shims to help with being conditionally [`Sync`].

#[cfg(not(feature = "sync"))]
pub(crate) use not_sync::*;
#[cfg(feature = "sync")]
pub(crate) use sync::*;

#[cfg(feature = "sync")]
mod sync {
    macro_rules! maybe_send_impl_future {
        ($t:ty) => { impl Future<Output = $t> + Send + 'static }
    }
    pub(crate) use maybe_send_impl_future;
}

#[cfg(not(feature = "sync"))]
mod not_sync {
    macro_rules! maybe_send_impl_future {
        ($t:ty) => { impl Future<Output = $t> + 'static }
    }
    pub(crate) use maybe_send_impl_future;
}
