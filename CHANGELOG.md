# Changelog

## 0.1.3 (2026-09-25)

- With `fork = true` or a `timeout` (which implies fork), the test cases run in child processes that the crate cannot see. It used to report 0 passing test cases and a useless failure bound of 1. It now takes n from proptest's own count, so the failure bound is right, and reports coverage as `not measured: fork mode`. The record has `"fork": true` and `"counts_agree": null`.

## 0.1.2 (2026-09-25)

- Fixed: when a persisted failure was replayed and failed again, the runner's shrinking calls that passed were counted as new passing test cases in `n` (reported by a user). Only failing runs were affected; the numbers for passing tests were not.
- `counts_agree` now compares `n` with proptest's `successes` whether or not the test passed, so a miscount like this one shows up.

## 0.1.1 (2026-09-25)

- The usage example at the top of the documentation is now a complete example that the doc tests compile and run, instead of an untested snippet.

## 0.1.0 (2026-09-25)

- `proptest!` accepting all twelve forms of proptest's macro; parsing is handed to proptest's `proptest_helper!`, so proptest is pinned to `>=1.10, <1.12`.
- For each passing test: the 95% upper bound on the failure probability and the passing test cases needed to bring it below a target; with the `coverage` feature, the regions of the code under test the test cases ran and the chance that the next test case runs a region none has run.
- Coverage from LLVM's counters and the coverage mapping in the test binary (LLVM 19–22, rustc 1.86–1.98); an unreadable layout is reported as `coverage_error`, and the chance of new code is then counted in counters.
- The numbers are computed in an array indexed by region or counter, not a hash map: in debug builds the time after a campaign of 5,256 test cases fell from 0.003–2.5 s to 0.001–0.18 s on eleven real crates.
- `RESIDUAL_RISK_COVERAGE_EVERY=k` takes coverage for every k-th test case only; the record gives `coverage_every` and `n_coverage`.
- The record gives `mapping_s`, the time to read the coverage mapping, and `compute_s`, the time to compute the numbers after the run.
- By default only this crate's own `src/` is left out of the code under test, not everything under its directory.
- A demo crate (`demo/`), the README's terminal recording (`demo/demo.tape`, `assets/`), and CI on Linux and macOS.
- Documentation split into a short README and `docs/theory.md`, `docs/evidence.md` (with figures) and `docs/reference.md`; the README opens with the demo's output without and with the crate, side by side (`demo/make_showcase.py`).
