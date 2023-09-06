use core::panic::Location;

/// Information available to a progress callback.
pub struct ProgressInfo<'a> {
    pub(crate) fraction: f32,
    pub(crate) label: &'a str,
    pub(crate) location: &'a Location<'a>,
}
impl<'a> ProgressInfo<'a> {
    /// The fraction of overall progress that has been made; always at least 0 and at most 1.
    ///
    /// The value might be less than previously reported values; for example, if the
    /// amount of remaining work is only estimated and the estimate has changed.
    pub fn fraction(&self) -> f32 {
        self.fraction
    }

    /// A label for the current portion of work.
    pub fn label_str(&self) -> &str {
        // This function is called `label_str()`, and does not return`&'a str`, to leave room for a
        // non-string label being offered as a non-breaking change.
        self.label
    }

    /// Source code location which reported the progress.
    pub fn location(&self) -> &'a Location<'a> {
        self.location
    }
}

/// Information available to a yield callback.
pub struct YieldInfo<'a> {
    pub(crate) location: &'a Location<'a>,
}

impl<'a> YieldInfo<'a> {
    /// Source code location which yielded.
    pub fn location(&self) -> &'a Location<'a> {
        self.location
    }
}
