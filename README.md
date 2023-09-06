`YieldProgress`
===============

This library, `yield-progress`, provides the `YieldProgress` type, which allows a long-running async task to report its progress, while also yielding to the scheduler (e.g. for the single-threaded web/Wasm environment) and introducing cancellation points.

These things go together because the rate at which it makes sense to yield (to avoid event
loop hangs) is similar to the rate at which it makes sense to report progress,
at least for human interfaces.

Compatibility
-------------

* `YieldProgress` is executor-independent; when it is constructed, the caller may provide a function
  for yielding using executor-specific mechanisms such as `tokio::task::yield_now()`,
  or may use the built-in simple yielder.
* The library is `no_std` compatible if default features are disabled.
  See the library documentation for details on the consequences.

Project status and stability
----------------------------

`yield-progress` has been split out of my larger project [`all-is-cubes`](https://crates.io/crates/all-is-cubes) so that I can use it for other applications; its functionality and applicability may be limited, but I believe it is free of bugs.

License
-------

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

Contribution
------------

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.