use alloc::sync::Arc;
use alloc::vec::Vec;
use std::sync::Mutex;

use crate::{ProgressInfo, YieldProgress};

/// Aggregate progress information from multiple concurrent tasks.
pub(crate) struct ConcurrentProgress {
    parent: YieldProgress,
    children: Mutex<Vec<Child>>,
}

#[derive(Default)]
struct Child {
    fraction: f32,
    label: Option<crate::Label>,
}

impl ConcurrentProgress {
    pub(crate) fn new(parent: YieldProgress, count: usize) -> Arc<Self> {
        Arc::new(Self {
            parent,
            children: Mutex::new(
                core::iter::repeat_with(Default::default)
                    .take(count)
                    .collect(),
            ),
        })
    }

    pub(crate) fn progressor(self: Arc<Self>, index: usize) -> impl Fn(&ProgressInfo<'_>) {
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
                .find_map(|child_state| child_state.label.as_ref());

            // Deliver combined results.
            self.parent
                .send_progress(fraction, first_incomplete_label, location);
        }
    }
}
