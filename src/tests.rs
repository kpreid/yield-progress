use super::*;

use std::prelude::rust_2021::*;

use tokio::sync::mpsc::{self, error::TryRecvError};

fn assert_send_sync<T: Send + Sync>() {
    // We don't need to do anything in this function; the call to it having been successfully
    // compiled is the assertion.
}

/// Something that the [`YieldProgress`] under test did.
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Yielded,
    Progress(f32, String),
    /// The receiver was dropped
    Dropped,
}
use Entry::*;

struct YpLog(mpsc::UnboundedReceiver<Entry>);

fn logging_yield_progress() -> (YieldProgress, YpLog) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let yp = YieldProgress::new(
        {
            let sender = sender.clone();
            move || {
                let _ = sender.send(Entry::Yielded);
                std::future::ready(())
            }
        },
        move |progress, label| drop(sender.send(Entry::Progress(progress, label.to_owned()))),
    );
    (yp, YpLog(receiver))
}

impl YpLog {
    fn drain(&mut self) -> Vec<Entry> {
        let mut entries = Vec::new();
        loop {
            match self.0.try_recv() {
                Ok(entry) => entries.push(entry),
                Err(TryRecvError::Empty) => return entries,
                Err(TryRecvError::Disconnected) => {
                    entries.push(Dropped);
                    return entries;
                }
            }
        }
    }
}

#[test]
fn yield_progress_is_sync() {
    assert_send_sync::<YieldProgress>()
}

#[tokio::test]
async fn basic_progress() {
    // Construct instance. Nothing happens immediately.
    let (p, mut r) = logging_yield_progress();
    assert_eq!(r.drain(), vec![]);

    // Simple progress.
    let progress_future = p.progress(0.25);
    assert_eq!(r.drain(), vec![Progress(0.25, "".into())]);
    progress_future.await;
    assert_eq!(r.drain(), vec![Yielded]);
}

#[tokio::test]
async fn progress_without_yield() {
    let (p, mut r) = logging_yield_progress();
    assert_eq!(r.drain(), vec![]);

    p.progress_without_yield(0.25);
    assert_eq!(r.drain(), vec![Progress(0.25, "".into())]);
}

#[tokio::test]
async fn yield_without_progress() {
    let (p, mut r) = logging_yield_progress();
    assert_eq!(r.drain(), vec![]);

    let future = p.yield_without_progress();
    assert_eq!(r.drain(), vec![]);
    future.await;
    assert_eq!(r.drain(), vec![Yielded]);
}

#[tokio::test]
async fn set_label() {
    let (mut p, mut r) = logging_yield_progress();
    p.set_label("hello");
    assert_eq!(r.drain(), vec![]); // TODO: labels should take effect sooner, or should they?
    p.progress(0.25).await;
    assert_eq!(r.drain(), vec![Progress(0.25, "hello".into()), Yielded]);
}

#[tokio::test]
async fn finish() {
    let (p, mut r) = logging_yield_progress();
    p.finish().await;
    assert_eq!(r.drain(), vec![Progress(1.0, "".into()), Yielded, Dropped]);
}

#[tokio::test]
async fn finish_and_cut() {
    let (p, mut r) = logging_yield_progress();
    let p2 = p.finish_and_cut(0.5).await;
    assert_eq!(r.drain(), vec![Progress(0.5, "".into()), Yielded]);
    p2.progress(0.5).await;
    assert_eq!(r.drain(), vec![Progress(0.75, "".into()), Yielded]);
}

#[tokio::test]
async fn start_and_cut() {
    let (mut p, mut r) = logging_yield_progress();

    let piece = p.start_and_cut(0.5, "part 1").await;
    assert_eq!(r.drain(), vec![Progress(0.0, "part 1".into()), Yielded]);

    // The cut off piece is the first half.
    piece.finish().await;
    assert_eq!(r.drain(), vec![Progress(0.5, "part 1".into()), Yielded]);

    // `p` is left with the remaining second half.
    p.progress(0.5).await;
    assert_eq!(r.drain(), vec![Progress(0.75, "".into()), Yielded]);
    p.finish().await;
    assert_eq!(r.drain(), vec![Progress(1.0, "".into()), Yielded, Dropped]);
}

#[tokio::test]
async fn split_evenly_basic() {
    let (p, mut r) = logging_yield_progress();
    let [p1, p2, p3, mut p4] = <[_; 4]>::try_from(p.split_evenly(4).collect::<Vec<_>>()).unwrap();

    p1.finish().await;
    assert_eq!(r.drain(), vec![Progress(0.25, "".into()), Yielded]);

    // Ignore p2 and check what happens when we move on to p3.
    drop(p2);
    p3.progress(0.5).await;
    assert_eq!(r.drain(), vec![Progress(5. / 8., "".into()), Yielded]);
    p3.finish().await;
    assert_eq!(r.drain(), vec![Progress(6. / 8., "".into()), Yielded]);

    p4.set_label("hello");
    p4.finish().await;
    assert_eq!(
        r.drain(),
        vec![Progress(1.0, "hello".into()), Yielded, Dropped]
    );
}

/// Test that `split_evenly()`'s arithmetic works okay at `usize::MAX`.
///
/// (This is a trivial test on 64-bit platforms because [`f32`] does not have enough resolution
/// to discriminate.)
#[tokio::test]
async fn split_evenly_with_max() {
    let (mut p, mut r) = logging_yield_progress();
    p = p.split_evenly(usize::MAX).next_back().unwrap();
    p.progress(0.0).await;
    assert_eq!(
        r.drain(),
        vec![
            Progress(1.0 - (usize::MAX as f32).recip(), "".into()),
            Yielded
        ]
    );
    p.finish().await;
    assert_eq!(r.drain(), vec![Progress(1.0, "".into()), Yielded, Dropped]);
}

// TODO: test split()
