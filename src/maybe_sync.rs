//! Shims to help with being conditionally [`Sync`].

#[cfg(not(feature = "sync"))]
pub(crate) use not_sync::*;
#[cfg(feature = "sync")]
pub(crate) use sync::*;

#[cfg(feature = "sync")]
mod sync {
    pub(crate) use std::sync::Arc as MaRc;
    pub(crate) use std::sync::Mutex as StateCell;

    pub(crate) trait Mutexish {}

    macro_rules! maybe_send_impl_future {
        ($t:ty) => { impl Future<Output = $t> + Send + 'static }
    }
    pub(crate) use maybe_send_impl_future;
}

#[cfg(not(feature = "sync"))]
mod not_sync {
    pub(crate) use alloc::rc::Rc as MaRc;
    pub(crate) use core::cell::RefCell as StateCell;

    /// For internal convenience, make `RefCell` look like `Mutex`.
    /// This trait is only used as an extension trait for the method, not generically.
    pub(crate) trait Mutexish {
        type WriteGuard<'a>
        where
            Self: 'a;
        type Error;
        fn lock(&self) -> Result<Self::WriteGuard<'_>, Self::Error>;
    }
    impl<T> Mutexish for std::cell::RefCell<T> {
        type WriteGuard<'a> = std::cell::RefMut<'a, T> where T: 'a;
        type Error = std::cell::BorrowMutError;

        fn lock(&self) -> Result<Self::WriteGuard<'_>, Self::Error> {
            self.try_borrow_mut()
        }
    }

    macro_rules! maybe_send_impl_future {
        ($t:ty) => { impl Future<Output = $t> + 'static }
    }
    pub(crate) use maybe_send_impl_future;
}
