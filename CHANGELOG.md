# Changelog

## Unreleased

## 0.1.2 (2023-09-05)

Updated README for accuracy regarding the changes in v0.1.1.

## 0.1.1 (2023-09-05)

### Added

* `basic_yield_now()` is a yield function that may be adequate rather than writing your own.
* `Builder` is a builder for `YieldProgress` instances.
* `ProgressInfo` and `YieldInfo` offer information to the callback functions.
* Feature `sync` may be disabled to avoid requiring `std::sync`.
* With all features disabled, the crate is now `no_std` compatible.

### Changed

The functions `YieldProgress::new()` and `YieldProgress::noop()` have been deprecated
in favor of using the `Builder`. The builder also differs in the following ways:
  
* `Builder::new().build()` uses `basic_yield_now()` rather than not yielding at all.
* The progress and yield callbacks are given `&ProgressInfo` and `&YieldInfo` structs.

### Removed

* No logging is done unless the `log_hiccups` feature is enabled.

## 0.1.0 (2023-08-23)

Initial public release.
