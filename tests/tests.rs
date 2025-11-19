use tokio::sync::mpsc::{self, error::TryRecvError};

use yield_progress::{Builder, YieldProgress};

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
    let yp = Builder::new()
        .yield_using({
            let sender = sender.clone();
            move |_| {
                let _ = sender.send(Entry::Yielded);
                core::future::ready(())
            }
        })
        .progress_using(move |info| {
            drop(sender.send(Entry::Progress(
                info.fraction(),
                info.label_str().to_owned(),
            )))
        })
        .build();
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

    let piece_1 = p.start_and_cut(0.25, "part 1").await;
    assert_eq!(r.drain(), vec![Progress(0.0, "part 1".into()), Yielded]);

    // The cut off piece is the first half.
    piece_1.finish().await;
    assert_eq!(r.drain(), vec![Progress(0.25, "part 1".into()), Yielded]);

    // Cut another piece, checking the starting point
    let piece_2 = p.start_and_cut(0.5, "part 2").await;
    assert_eq!(r.drain(), vec![Progress(0.25, "part 2".into()), Yielded]);
    piece_2.finish().await;
    assert_eq!(r.drain(), vec![Progress(0.625, "part 2".into()), Yielded]);

    // `p` is left with the remaining progress.
    p.progress(0.5).await;
    assert_eq!(r.drain(), vec![Progress(0.8125, "".into()), Yielded]);
    p.finish().await;
    assert_eq!(r.drain(), vec![Progress(1.0, "".into()), Yielded, Dropped]);
}

/// Test that start_and_cut() doesn't require its label to outlive the future.
/// This is a “does it compile?” test that does not need to be run.
async fn _start_and_cut_label_is_not_captured() {
    let mut p = YieldProgress::noop();
    let future = {
        let s = String::from("hello");
        p.start_and_cut(0.5, &s)
    };
    future.await;
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

#[cfg(feature = "sync")]
#[tokio::test]
async fn split_evenly_concurrent() {
    let (p, mut r) = logging_yield_progress();
    let mut iter = p.split_evenly_concurrent(2);
    let mut p1 = iter.next().unwrap();
    let mut p2 = iter.next().unwrap();
    p1.set_label("p1");
    p2.set_label("p2");
    p1.progress(0.5).await;
    p2.progress(0.5).await;
    p1.finish().await;
    p2.finish().await;
    // TODO: we should have a better rule for combining labels,
    // or give more information to the callback.
    // This test is looking for "first nonempty nonfinished label".
    assert_eq!(
        r.drain(),
        vec![
            Progress(0.25, "p1".into()),
            Yielded,
            Progress(0.5, "p1".into()),
            Yielded,
            Progress(0.75, "p2".into()),
            Yielded,
            Progress(1.0, "".into()),
            Yielded,
        ]
    );
}

#[tokio::test]
async fn basic_yield_smoke_test() {
    yield_progress::basic_yield_now().await;
}

// TODO: test split()
