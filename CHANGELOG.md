# Changelog

## Unreleased

### Added

* `basic_yield_now()` is a yield function that may be adequate rather than writing you own.
* `Builder` is a builder for `YieldProgress` instances.

  The existing functions `YieldProgress::new()` and `YieldProgress::noop()` have been deprecated
  in favor of the builder.
  Note that `Builder::new().build()` uses `basic_yield_now()` rather than not yielding at all.

## 0.1.0 (2023-08-23)

Initial public release.
