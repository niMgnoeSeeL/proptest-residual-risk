# Changelog

## 0.1.0 (unreleased)

- `proptest!` accepting all twelve forms of proptest's macro; parsing is handed to proptest's `proptest_helper!`, so proptest is pinned to `>=1.10, <1.12`.
- For each passing test: the 95% upper bound on the failure probability and the passing test cases needed to bring it below a target; with the `coverage` feature, the regions of the code under test the test cases ran and the chance that the next test case runs a region none has run.
- Coverage from LLVM's counters and the coverage mapping in the test binary (LLVM 19–22, rustc 1.86–1.98); an unreadable layout is reported as `coverage_error`, and the chance of new code is then counted in counters.
- The numbers are computed in an array indexed by region or counter, not a hash map: in debug builds the time after a campaign of 5,256 test cases fell from 0.003–2.5 s to 0.001–0.18 s on eleven real crates.
- `RESIDUAL_RISK_COVERAGE_EVERY=k` takes coverage for every k-th test case only; the record gives `coverage_every` and `n_coverage`.
- The record gives `mapping_s`, the time to read the coverage mapping, and `compute_s`, the time to compute the numbers after the run.
