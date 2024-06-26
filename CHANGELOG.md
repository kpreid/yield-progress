# Changelog

## Unreleased

## 0.1.7 (2024-06-30)

* Reduced the size of futures produced by the library, for efficiency.

## 0.1.6 (2024-04-28)

* Added `YieldProgress::split_evenly_concurrent()` for reporting progress from concurrent tasks.

## 0.1.5 (2023-12-25)

* `YieldProgress::noop()` is no longer deprecated.
* Replaced the optional `instant` dependency with `web-time`.
  If you previously found it necessary to declare the `instant/wasm-bindgen` feature,
  you will no longer need to do this.

## 0.1.4 (2023-09-30)

* Fixed `YieldProgress::start_and_cut()` always using zero as the start position.

## 0.1.3 (2023-09-05)

* Fixed `ProgressInfo::label_str()` not being public.

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
