use core::fmt;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

#[cfg(doc)]
use super::YieldProgress;

/// The minimum, executor-agnostic yield operation.
/// **This may be unsuitable for some applications.**
///
/// This function is provided as a convenience for constructing a [`YieldProgress`] where no more
/// specific yield implementation is required. It does not itself interact with the
/// [`YieldProgress`] system.
///
/// # Caveat
///
/// This function implements yielding by returning a [`Future`] which will:
///
/// 1. The first time it is polled, immediately [wake] and return [`Poll::Pending`].
/// 2. The second time it is polled, return [`Poll::Ready`].
///
/// This might be inadequate if the executor's scheduling policy:
///
/// * Distinguishes intentional yielding.
///   For example, in Tokio 1.\*, you should use [`tokio::task::yield_now()`] instead.
/// * Is not fair among tasks, so some amount of delay is required to successfully yield to other
///   tasks.
/// * Is not fair between tasks and something else.
///   For example, if the executor is implemented inside some event loop but itself loops through
///   Rust async tasks as long as any of the tasks have [woken][wake], then something additional is
///   needed to yield to the higher level.
///
/// [wake]: core::task::Waker::wake()
/// [`tokio::task::yield_now()`]: https://docs.rs/tokio/1/tokio/task/fn.yield_now.html
pub fn basic_yield_now() -> impl Future<Output = ()> + fmt::Debug + Send + 'static {
    BasicYieldNow { state: State::New }
}

#[derive(Debug)]
struct BasicYieldNow {
    state: State,
}

#[derive(Clone, Copy, Debug)]
enum State {
    New,
    Ready,
    Used,
}
impl Future for BasicYieldNow {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = &mut self.get_mut().state;
        match *state {
            State::New => {
                *state = State::Ready;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            State::Ready => {
                *state = State::Used;
                Poll::Ready(())
            }
            State::Used => panic!("future polled after completion"),
        }
    }
}
