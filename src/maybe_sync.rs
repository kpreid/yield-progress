//! Shims to help with being conditionally [`Sync`].

#[cfg(not(feature = "sync"))]
pub(crate) use not_sync::*;
#[cfg(feature = "sync")]
pub(crate) use sync::*;

#[cfg(feature = "sync")]
mod sync {

    pub(crate) use std::sync::Mutex as StateCell;

    // stub definition; the real one is only used when the feature is disabled
    #[allow(dead_code)]
    pub(crate) trait Mutexish {}

    macro_rules! maybe_send_impl_future {
        ($t:ty) => { impl Future<Output = $t> + Send + 'static }
    }
    pub(crate) use maybe_send_impl_future;
}

#[cfg(not(feature = "sync"))]
mod not_sync {
    pub(crate) use core::cell::RefCell as StateCell;

    /// For internal convenience, make `RefCell` look like `Mutex`.
    ///
    /// This trait is only used as an extension trait for the method, not generically.
    /// That is less code and lets us avoid use of a generic associated type, which is required
    /// for our MSRV of 1.63 < 1.65.
    pub(crate) trait Mutexish {
        type Target;
        fn lock(&self) -> Result<core::cell::RefMut<'_, Self::Target>, core::cell::BorrowMutError>;
    }
    impl<T> Mutexish for core::cell::RefCell<T> {
        type Target = T;
        fn lock(&self) -> Result<core::cell::RefMut<'_, T>, core::cell::BorrowMutError> {
            self.try_borrow_mut()
        }
    }

    macro_rules! maybe_send_impl_future {
        ($t:ty) => { impl Future<Output = $t> + 'static }
    }
    pub(crate) use maybe_send_impl_future;
}
