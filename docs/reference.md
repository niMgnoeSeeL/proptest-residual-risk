# Reference

[← README](../README.md) · [Theory](theory.md) · [Evidence](evidence.md) · **Reference**

- [Configuration](#configuration)
- [The record file](#the-record-file)
- [How it works](#how-it-works)
- [Known limitations](#known-limitations)
- [Supported versions](#supported-versions)
- [Running this crate's tests](#running-this-crates-tests)

## Configuration

All settings are environment variables. Their names start with `RESIDUAL_RISK`, not `PROPTEST_`, because proptest warns about every `PROPTEST_` variable it does not know.

| Variable | Meaning | Default |
| --- | --- | --- |
| `RESIDUAL_RISK=1` | Print the numbers in each test's output (shown with `cargo test -- --nocapture` or `--show-output`) | off |
| `RESIDUAL_RISK_TARGET` | The failure probability for which the needed number of test cases is computed | `0.001` |
| `RESIDUAL_RISK_DIR` | Where the record files go | `<target>/<profile>/proptest-residual-risk/` |
| `RESIDUAL_RISK_CODE` | The code under test: comma-separated path prefixes; a prefix starting with `!` excludes (e.g. `/home/me/proj/src/,!/home/me/proj/src/bin/`) | every source file except dependencies, the standard library, `tests/`, `benches/`, `examples/` and this crate's `src/` |
| `RESIDUAL_RISK_TRACE=1` | Also write, for each passing test case, the counters and regions it ran | off |
| `RESIDUAL_RISK_COVERAGE_EVERY` | Take coverage for every k-th generated test case only. The chance of new code is then computed from those test cases, so it describes fewer test cases and reads higher; the failure bound still uses all of them | `1` |

## The record file

Every test run appends one JSON line to `<test name>.jsonl` in the records directory. This happens whether or not `RESIDUAL_RISK=1` is set: without it nothing is printed, but the record is still written.

| Field | Meaning |
| --- | --- |
| `test`, `unix_time`, `passed` | Which test, when, and whether it passed |
| `n`, `runner_successes`, `counts_agree`, `fork` | Passing new test cases counted by the crate, proptest's `successes`, whether the two agree, and whether the test ran in fork mode (then `n` is proptest's count and `counts_agree` is null) |
| `rejects` | Test cases rejected by `prop_assume!` |
| `wall_s`, `body_s`, `t_s` | Wall time of the run, time spent in test bodies, and wall time per passing test case |
| `failure_bound`, `target`, `cases_for_target` | $1-0.05^{1/n}$, the target, and the passing test cases needed in total to go below it |
| `total_regions`, `distinct_regions` | Regions in the code under test, and how many the passing test cases ran |
| `region_singletons`, `k_regions`, `p_new_regions` | Singletons, $k$ and the chance of new code, counted in regions |
| `distinct_counters`, `singletons`, `k`, `p_new` | The same, counted in counters |
| `p_new_basis` | `"regions"`, or `"counters"` when regions could not be computed |
| `coverage_error` | Present when coverage could not be read, with the reason |
| `window_counters`, `coverage_every`, `n_coverage` | Counters copied per test case, the sampling setting, and how many passing test cases had coverage taken |
| `mapping_s`, `compute_s` | Time to read the coverage mapping (once per process), and to compute the numbers after the run |
| `code_files` | The source files counted as code under test |

## How it works

1. `proptest_residual_risk::proptest!` takes the same input as proptest's `proptest!` and hands the parsing of its twelve forms to proptest's own `proptest_helper!`. It runs the test through `TestRunner::run` as proptest does, wrapping the test body so that the crate sees every call.
2. The crate cannot ask the runner what kind of call it is making, so it infers it from the order of calls. Calls that replay persisted failures come first; the crate knows how many from `FailurePersistence::load_persisted_failures2`, as the runner does, and does not record them. After the first failure, including a replay that fails again, the runner is shrinking, and calls pass through unrecorded. In fork mode the calls happen in child processes, so the crate takes the count of passing test cases from the runner.
3. For each recorded test case the crate keeps its outcome (pass, reject, fail), its time and, with coverage, the counters and regions it ran.
4. With the `coverage` feature, it copies LLVM's counters (through `__llvm_profile_begin_counters`) before and after each test case, only those of functions in the code under test. Once per process it reads the coverage mapping from the running test binary's `__llvm_covmap` and `__llvm_covfun` sections (Mach-O or ELF, through the `object` crate), and uses the profile data records (`__llvm_prf_data`) to find each function's counters.
5. When the runner returns, the crate computes the three numbers, appends the JSON line and, with `RESIDUAL_RISK=1`, prints the block.

## Known limitations

- **LLVM 23.** LLVM 23 changed the layout of the profile data records. Rust nightlies from 2026-08-06 use it. There the crate cannot compute regions: the record gets a `coverage_error`, the output says `coverage: not measured: …` instead of showing zeros, and the chance of new code is counted in counters, ending with `(counted in counters: regions unavailable)`.
- **Debug builds only for coverage.** In an optimised build LLVM can merge or drop the counter updates of inlined code.
- **One test at a time.** LLVM's counters are shared by the whole process. Tests using this crate take turns through one lock, but other tests running at the same time would mix in. Use `--test-threads=1` or `cargo nextest`.
- **`fork = true` or a `timeout`** (which implies fork) runs test cases in child processes that the crate cannot see. The failure bound then uses proptest's own count of passing test cases and is still right; coverage and the chance of new code are reported as not measured.
- **Other entry points are not covered.** `#[property_test]`, test-strategy's `#[proptest]` and `prop_state_machine!` do not go through this crate's macro.
- **The macro relies on a hidden proptest macro** (`proptest_helper!`). A proptest release that changes it would break this crate until updated, which is why the supported proptest versions are pinned.

## Supported versions

- **Rust:** 1.86 or newer (`rust-version = "1.86"`).
- **Coverage:** LLVM 19–22, which is stable rustc 1.86–1.98.
- **proptest:** 1.10 and 1.11 (`>=1.10, <1.12`); each new release is checked before the range is widened.
- **Checked** on macOS arm64 with rustc 1.86.0 (LLVM 19), 1.87.0 (LLVM 20) and 1.98.1 (LLVM 22), proptest 1.10.0 and 1.11.0, and the lowest versions of `object`, `miniz_oxide` and `md5` that `Cargo.toml` allows. CI runs the tests with and without coverage on Linux and macOS with stable Rust, and on Linux with Rust 1.86.

## Running this crate's tests

```bash
cargo test -- --test-threads=1
RUSTFLAGS="-C instrument-coverage" cargo test --features coverage -- --test-threads=1
cargo llvm-cov --no-report --features coverage -- --test-threads=1
cd demo && cargo llvm-cov --no-report --features coverage -- --nocapture
```

The README's picture is drawn from the demo's real output by `cd demo && python3 make_showcase.py`; the terminal recording `assets/demo.gif` by `vhs demo/demo.tape`.
