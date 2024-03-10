use alloc::vec::Vec;

#[cfg(not(feature = "sync"))]
use crate::Mutexish as _;
use crate::{MaRc, ProgressInfo, StateCell, YieldProgress};

/// Aggregate progress information from multiple concurrent tasks.
pub(crate) struct ConcurrentProgress {
    parent: YieldProgress,
    children: StateCell<Vec<Child>>,
}

#[derive(Default)]
struct Child {
    fraction: f32,
    label: Option<MaRc<str>>,
}

impl ConcurrentProgress {
    pub(crate) fn new(parent: YieldProgress, count: usize) -> MaRc<Self> {
        MaRc::new(Self {
            parent,
            children: StateCell::new(
                core::iter::repeat_with(Default::default)
                    .take(count)
                    .collect(),
            ),
        })
    }

    pub(crate) fn progressor(self: MaRc<Self>, index: usize) -> impl Fn(&ProgressInfo<'_>) {
        move |info| {
            let &ProgressInfo {
                fraction,
                label,
                location,
            } = info;

            let children = &mut self.children.lock().unwrap();

            // Store new state.
            let child_state = &mut children[index];
            child_state.fraction = fraction;
            child_state.label = label.cloned();

            // Compute combined results.
            let sum: f32 = children
                .iter()
                .map(|child_state| child_state.fraction)
                .sum();
            let fraction = sum / (children.len() as f32);

            let first_incomplete_label = children
                .iter()
                .filter(|child_state| child_state.fraction < 1.0)
                .filter_map(|child_state| child_state.label.as_ref())
                .next();

            // Deliver combined results.
            self.parent
                .send_progress(fraction, first_incomplete_label, location);
        }
    }
}
