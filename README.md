# proptest-residual-risk

When a [proptest](https://github.com/proptest-rs/proptest) test passes, `cargo test` prints `ok` and nothing else. You learn that 256 test cases found no counterexample. You do not learn whether 256 was enough, how much of your code those test cases reached, or what `PROPTEST_CASES` should be.

This crate adds that information to every passing test, without changing how proptest generates or runs test cases. For each test it reports three numbers:

1. **Coverage:** how much of the code under test the passing test cases ran, counted in LLVM coverage regions.
2. **Chance of new code:** an estimate of the chance that the next test case runs code that no test case has run yet, and how many more test cases that is expected to take.
3. **Failure bound:** a 95% upper bound on the chance that the next test case fails, and how many passing test cases in total would bring it below a target you choose.

The failure bound needs nothing but the crate. Coverage and the chance of new code need a coverage build (`-C instrument-coverage`) and the crate's `coverage` feature.

> **Status.** Version 0.1.0, not yet published on crates.io. The code, tests and this document were written with the help of an AI assistant (Claude, by Anthropic). Every number in this document was measured with this crate; how is described under [Evidence](#evidence).

## Contents

- [Quick start](#quick-start)
- [What the output means](#what-the-output-means)
- [The three numbers](#the-three-numbers)
- [What the numbers do not say](#what-the-numbers-do-not-say)
- [Configuration](#configuration)
- [The record file](#the-record-file)
- [Evidence](#evidence)
- [Supported versions](#supported-versions)
- [Known limitations](#known-limitations)
- [How it works](#how-it-works)
- [Running this crate's tests](#running-this-crates-tests)
- [License](#license)
- [References](#references)

## Quick start

Add the crate next to proptest as a dev-dependency. Until it is on crates.io, take it from this repository:

```toml
[dev-dependencies]
proptest = "1.11"
proptest-residual-risk = { git = "https://github.com/niMgnoeSeeL/proptest-residual-risk" }
```

To also get coverage and the chance of new code, turn on the `coverage` feature:

```toml
proptest-residual-risk = { git = "https://github.com/niMgnoeSeeL/proptest-residual-risk", features = ["coverage"] }
```

In each test file, add one `use` line after proptest's prelude. It replaces proptest's `proptest!` macro with this crate's, which accepts exactly the same syntax. The tests stay as they are.

```rust
use proptest::prelude::*;
use proptest_residual_risk::proptest;

proptest! {
    #[test]
    fn clamp_in_range(x: i64, lo: i64, hi: i64) {
        prop_assume!(lo <= hi);
        let y = my_crate::clamp(x, lo, hi);
        prop_assert!(lo <= y && y <= hi);
    }
}
```

Run the tests. `RESIDUAL_RISK=1` prints the numbers; `--show-output` makes `cargo test` show the output of passing tests.

```bash
# the failure bound only; any build
RESIDUAL_RISK=1 cargo test -- --show-output

# all three numbers (recommended): under cargo-llvm-cov, one test at a time
cargo install cargo-llvm-cov   # once
RESIDUAL_RISK=1 cargo llvm-cov --no-report -- --test-threads=1 --show-output

# all three numbers without cargo-llvm-cov: instruments every crate, dependencies included
RUSTFLAGS="-C instrument-coverage" RESIDUAL_RISK=1 \
  cargo test -- --test-threads=1 --show-output
```

[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) instruments only your workspace's own crates, not dependencies such as proptest. The numbers are the same, and it is cheaper: on 11 real crates, 2.25× the time of plain proptest against 3.42× with `RUSTFLAGS` (geometric means; see [Evidence](#evidence)). Drop `--no-report` to also get cargo-llvm-cov's usual coverage report. With `cargo nextest` (`cargo llvm-cov nextest`), which runs every test in its own process, `--test-threads=1` is not needed. Without `RESIDUAL_RISK=1` nothing is printed, but every test still appends one line to a record file ([The record file](#the-record-file)).

## What the output means

This is real output for the test above. The code under test is a small library whose `src/lib.rs` holds two functions, `clamp` (the one the test calls) and `mean` (which it does not call). proptest 1.11.0, rustc 1.98.1, debug build, macOS.

```text
---- clamp_in_range stdout ----
proptest: 256 passing test cases
	successes: 256
	local rejects: 0
	global rejects: 294
		294 times at tests/with_crate.rs:9:9: lo <= hi
	coverage: 9 of 15 regions (60%) in the code under test, 3 counters
	new code: about 0.0039 chance per test case (next new code after ~258 more, ~0.047 s)
	failure: at most 0.012 chance per test case (95% confidence)
	         below 0.001 after 2,739 more (~0.500 s): PROPTEST_CASES=2995
```

Line by line:

- `256 passing test cases`, `successes: 256`, `local rejects`, `global rejects`: the same statistics block that proptest itself prints when a test fails. $n = 256$ is the number of newly generated test cases that passed. The 294 global rejects are inputs that `prop_assume!(lo <= hi)` discarded; they are not counted in $n$.
- `coverage: 9 of 15 regions (60%) in the code under test, 3 counters`: the library has 15 coverage regions (source ranges the compiler tracks). The 256 test cases ran 9 of them: all of `clamp`, none of `mean`. `3 counters` is the number of LLVM counters that went up; there are fewer counters than regions because LLVM computes some regions' counts from other counters (see [Coverage](#coverage)).
- `new code: about 0.0039 chance per test case`: the estimated chance that test case 257 runs a region none of the 256 ran. Here no region was run by exactly one test case, so the estimate is at its floor $1/(n+2) = 1/258$. `next new code after ~258 more` is $1/0.0039$, and `~0.047 s` is that many test cases at this test's average time per test case.
- `failure: at most 0.012 chance per test case (95% confidence)`: the 95% upper bound $1 - 0.05^{1/256} = 0.0116$ on the chance that one more test case from this strategy fails.
- `below 0.001 after 2,739 more (~0.500 s): PROPTEST_CASES=2995`: to bring that bound below 0.001 (the default target), 2,995 passing test cases are needed in total, 2,739 more than now, which at this test's speed takes about half a second. The crate only reports this; it never runs more test cases than `cases`.

## The three numbers

### Which test cases are counted

Only test cases that the runner newly generated and that passed. We write $n$ for their number. When a test passes, $n$ equals proptest's own `successes`, and the record says whether the two agree (`counts_agree`). Not counted:

- replays of persisted failures (`proptest-regressions` files), which the runner runs before new test cases;
- test cases rejected by `prop_assume!` (reported as `rejects`) or redrawn by `prop_filter`;
- shrinking, which only happens after a failure. When a test fails, nothing is estimated.

### Failure bound

If $n$ test cases, drawn independently from the same distribution, all passed, then

$$\bar p_{95} = 1 - 0.05^{1/n} \approx \frac{3}{n}$$

is an exact binomial 95% upper confidence bound on the probability that one more test case from that distribution fails. With proptest's default 256 cases it is 0.0116. Bringing it below a target $\varepsilon$ needs

$$N_\varepsilon = \left\lceil \frac{\ln 0.05}{\ln(1-\varepsilon)} \right\rceil$$

passing test cases in total; for $\varepsilon = 0.001$ that is 2,995.

**The assumption, and why proptest meets it.** The bound assumes the $n$ test cases are independent draws from one distribution. proptest draws every test case from a fresh seed (`TestRunner::run_in_process_with_replay` calls `gen_get_seed` and then `new_tree`). It does not mutate earlier inputs, avoid earlier inputs, or steer generation by coverage. So the passing test cases are independent draws from the strategy's distribution, restricted to inputs that pass `prop_assume!` and `prop_filter`. The bound is about that distribution. A proptest change that makes early test cases differ from later ones (for example the edge-bias proposal in proptest #515) would break the assumption.

**"Confidence 95%" means:** if you use this bound on many tests that passed, at most about 5% of the bounds will be below the true failure probability of their test. It does not mean a particular bound is right with probability 0.95.

### Chance of new code

Call a coverage element a *singleton* if exactly one of the $n$ passing test cases ran it. Let $k$ be the number of test cases that ran at least one singleton. The estimate is

$$\hat p_{\text{new}} = \max\left(\frac{k}{n},\ \frac{1}{n+2}\right).$$

- $k/n$ is Ma and Chao's estimator of the chance of seeing a new species, as adapted to code coverage by Lee and Böhme, *Dependency-aware Residual Risk Analysis* (ICSE 2026). One of the authors of that paper is behind this crate.
- $1/(n+2)$ is Laplace's rule of succession. It keeps the estimate from reading 0 after finitely many test cases.
- The expected number of test cases until new code runs is $1/\hat p_{\text{new}}$.

Example: three test cases ran the regions $\{a, b, c\}$, $\{a, b\}$ and $\{a, d, e\}$. The singletons are $c$, $d$ and $e$. Test cases 1 and 3 ran a singleton, so $k = 2$ and $\hat p_{\text{new}} = \max(2/3, 1/5) = 2/3$.

**This is an estimate, not a bound.** It is right on average but can be below the truth in a given run (see [Evidence](#evidence)); the output says "about" for this reason.

### Coverage

With the `coverage` feature and `-C instrument-coverage`, the crate reads LLVM's coverage counters before and after each test case's body, and so knows which counters each test case raised. It also reads the *coverage mapping* that the compiler stores in the test binary: for each function, its source regions and how each region's execution count follows from the counters. A region ran in a test case when its count went up.

Why regions and not counters: LLVM gives no counter of its own to code whose count it can compute from other counters. In

```rust
fn clamp(x: i64, lo: i64, hi: i64) -> i64 {
    if x < lo { return lo; }   // counter B
    if x > hi { return hi; }   // counter C
    x                          // no counter: count = entry (A) - B - C
}
```

a test case that returns `x` unchanged raises only the entry counter A. After test cases through `return lo` and `return hi`, a first test case through `x` raises no new counter, but it does run a new region. On 57 real crates, counting regions found 14% more test cases that ran new code than counting counters. The chance of new code is therefore counted in regions. If the regions cannot be computed (see [Known limitations](#known-limitations)), it falls back to counters and says so.

Both counts are limited to the *code under test*. By default that is every source file except dependencies (`~/.cargo/registry`, `~/.cargo/git`), the standard library, files under `tests/`, `benches/` and `examples/`, and this crate; `RESIDUAL_RISK_CODE` changes it.

## What the numbers do not say

- **All probabilities are over the test's strategy**, not over the inputs your code meets in production. If the strategy never generates an input, no number here says anything about it.
- **The failure bound is not the chance that your code has a bug.** It is the chance that one more test case from this strategy would fail.
- **The chance of new code does not bound the chance of failure.** A wrong result can come from code that every test case already ran. For example:

```rust
// defect: wrong result when a == 7
pub fn mean(a: u8, b: u8) -> u8 {
    if a != 7 { ((a as u16 + b as u16) / 2) as u8 } else { a.max(b) + 1 }
}

proptest! {
    #[test]
    fn mean_between(a in 0u8..=100, b in 0u8..=100) {
        let m = mean(a, b);
        prop_assert!(a.min(b) <= m && m <= a.max(b));
    }
}
```

The true failure probability is $1/101 \approx 0.0099$. With `PROPTEST_RNG_SEED=12` (proptest 1.11.0) the test passes and no passing test case runs the `else` branch. The chance of new code is 0.0039, below the truth; the failure bound is 0.0116, above it. Use the failure bound for "how likely is a failure", and the chance of new code for "is my strategy still finding new behaviour".

## Configuration

All settings are environment variables. Their names start with `RESIDUAL_RISK`, not `PROPTEST_`, because proptest warns about every `PROPTEST_` variable it does not know.

| Variable | Meaning | Default |
| --- | --- | --- |
| `RESIDUAL_RISK=1` | Print the numbers to each test's output (`cargo test -- --show-output` shows it) | off |
| `RESIDUAL_RISK_TARGET` | The failure probability for which the needed number of test cases is computed | `0.001` |
| `RESIDUAL_RISK_DIR` | Where the record files go | `<target>/<profile>/proptest-residual-risk/` |
| `RESIDUAL_RISK_CODE` | The code under test: comma-separated path prefixes; a prefix starting with `!` excludes (e.g. `src/,!src/bin/`) | see [Coverage](#coverage) |
| `RESIDUAL_RISK_TRACE=1` | Also write, for each passing test case, the counters and regions it ran (for checking the estimate) | off |
| `RESIDUAL_RISK_COVERAGE_EVERY` | Take coverage for every k-th generated test case only; the chance of new code is then computed from those test cases, so it describes fewer test cases and reads higher. The failure bound still uses all of them. On 11 crates, k = 4 cost 2.44× and k = 16 cost 2.15× of plain proptest, against 3.42× for k = 1 (all with `RUSTFLAGS`) | `1` |

## The record file

Every test run appends one JSON line to `<test name>.jsonl` in the records directory, whether or not `RESIDUAL_RISK=1` is set. The main fields:

| Field | Meaning |
| --- | --- |
| `test`, `unix_time`, `passed` | Which test, when, and whether it passed |
| `n`, `runner_successes`, `counts_agree` | Passing new test cases counted by the crate, proptest's `successes`, and whether the two agree |
| `rejects` | Test cases rejected by `prop_assume!` |
| `wall_s`, `body_s`, `t_s` | Wall time of the run, time spent in test bodies, and wall time per passing test case |
| `failure_bound`, `target`, `cases_for_target` | $1-0.05^{1/n}$, the target, and $N_\varepsilon$ |
| `total_regions`, `distinct_regions` | Regions in the code under test, and how many the passing test cases ran |
| `region_singletons`, `k_regions`, `p_new_regions` | Singletons, $k$ and $\hat p_{\text{new}}$ counted in regions |
| `distinct_counters`, `singletons`, `k`, `p_new` | The same counted in counters |
| `p_new_basis` | `"regions"`, or `"counters"` when regions could not be computed |
| `coverage_error` | Present when coverage could not be read, with the reason |
| `window_counters`, `coverage_every`, `n_coverage` | Counters copied per test case, the sampling setting, and how many passing test cases had coverage taken |
| `mapping_s`, `compute_s` | Time to read the coverage mapping (once per process) and to compute the numbers after the run |
| `code_files` | The source files counted as code under test |

## Evidence

All measurements below were made with this crate, proptest 1.11.0 and rustc 1.98.1 on macOS (arm64), in September 2026.

**The failure bound holds as promised.** We used 28 (bug, property) pairs from the Rust workloads of Etna (Shi et al., 2023): binary search tree, red-black tree and simply typed lambda calculus, each with injected bugs. For each pair we measured the true failure probability from independent draws (up to 100 failures or $10^7$ draws), then ran 300 campaigns per pair at three numbers of test cases, 25,200 campaigns in all. The bound can only be wrong in a campaign that passes although the property can fail. At the size where such passing campaigns are common (pass probability 0.049 under independent draws), 383 of 8,400 campaigns passed with a bound below the true probability: 0.0456 (95% interval 0.0412–0.0503), within the promised 5%. In all 84 (pair, size) combinations the observed pass rate matched the prediction for independent draws, and in all 25,200 campaigns the crate's $n$ equalled proptest's `successes`.

**The chance of new code is right on average, but it is not a bound.** On 199 properties of 57 real Rust crates (url, smallvec, itertools, toml_edit, regex and others), 20 seeds each, we computed the estimate from the first 256 passing test cases and compared it with the fraction of the next 5,000 that ran a region the first 256 had not. In the 60 properties where the estimate was above its floor in some seed (1,196 campaigns), the mean estimate was 0.0077 and the mean observed fraction 0.0060. But in 220 of those campaigns (18%) the estimate was below the lower end of the observed fraction's 95% confidence interval. In the other 139 properties the estimate stayed at its floor $1/258$ and the later test cases almost never ran new code (mean observed fraction 0.00001). Counting counters instead of regions gave the same picture but missed 14% of the test cases that ran new code (30,957 against 36,173 of 19.9 million).

**The cost.** On 11 properties of those crates, 5,256 passing test cases per campaign, compared with proptest's own macro:

| Setting | Time relative to proptest alone: geometric mean (range) over 11 properties |
| --- | --- |
| This crate's macro, no coverage | 1.03× (0.99–1.26×) |
| Coverage on, under `cargo llvm-cov --no-report` (only the workspace instrumented) | 2.25× (1.09–7.14×) |
| Coverage on, with `RUSTFLAGS="-C instrument-coverage"` (every crate instrumented) | 3.42× (1.10–28.39×) |
| For comparison: plain proptest under `cargo llvm-cov`, test run only / with its report | 1.08× (0.99–1.46×) / 2.70× (1.07–68.75×) |

Where the time goes with `RUSTFLAGS`: for the 8 crates whose tests build unoptimised, instrumentation itself costs almost nothing and most of the extra time is reading and comparing the counters of the code under test before and after each test case (0.05–3.55× of plain proptest). For the 3 crates whose manifests build tests at opt-level 3, instrumenting proptest and the other dependencies blocks optimisation (2.7–20× of plain); under cargo-llvm-cov, which leaves dependencies uninstrumented, these fall from 28.4× to 3.1× (base64), 17.6× to 7.1× (regex-syntax) and 10.1× to 4.3× (regex), with the same regions counted. Reading the coverage mapping takes 0.009–0.07 s once per process, and computing the numbers after a campaign of 5,256 test cases 0.001–0.18 s. Building tests in release mode does not help: relative to plain proptest in release mode, coverage costs 16.7× and optimisation drops some counter updates.

**Optimised builds gave the same numbers here.** Three of the crates build their tests at opt-level 3. Rerunning them in debug builds (160 campaigns) gave the same estimates and observed fractions in every campaign, although the debug builds saw up to 7% more regions.

## Supported versions

- **Rust:** 1.86 or newer (`rust-version = "1.86"`).
- **Coverage:** LLVM 19–22, which is stable rustc 1.86–1.98.
- **proptest:** 1.10 and 1.11 (`>=1.10, <1.12`). The macro relies on proptest's hidden `proptest_helper!`, so each new proptest release is checked before the range is widened.
- **Checked on** macOS arm64 with rustc 1.86.0 (LLVM 19), 1.87.0 (LLVM 20) and 1.98.1 (LLVM 22), proptest 1.10.0 and 1.11.0, and the lowest versions of `object`, `miniz_oxide` and `md5` that `Cargo.toml` allows. Linux (ELF binaries) is supported by the code but has not been tested yet.

## Known limitations

- **LLVM 23.** LLVM 23 changed the layout of the profile data records that tell which counters belong to which function. Rust nightlies from 2026-08-06 use it. There the crate cannot compute regions: the record gets a `coverage_error`, the output says `coverage: not measured: …` instead of showing zeros, and the chance of new code is counted in counters, ending with `(counted in counters: regions unavailable)`.
- **Measure coverage in debug builds** (the default for `cargo test` and `cargo llvm-cov`). In an optimised build LLVM can merge or drop the counter updates of inlined code; a call of `clamp` with constant arguments raised no counter at all.
- **One test at a time.** LLVM's counters are shared by the whole process. Tests using this crate take turns through one lock, but other tests running at the same time would mix in. Use `--test-threads=1`, or `cargo nextest`.
- **`fork = true`** runs test cases in child processes, whose records are lost.
- **Other entry points are not covered.** `#[property_test]`, test-strategy's `#[proptest]` and `prop_state_machine!` do not go through this crate's macro.
- **The macro relies on a hidden proptest macro** (`proptest_helper!`) to parse its twelve syntax forms. A proptest release that changes it would break this crate until updated; the supported range is pinned for that reason.

## How it works

- `proptest_residual_risk::proptest!` takes the same input as proptest's `proptest!` and hands the parsing to proptest's own `proptest_helper!`. It then runs the test through `TestRunner::run` as proptest does, wrapping the test body so that the crate sees every call.
- Calls that replay persisted failures are recognised by counting them with `FailurePersistence::load_persisted_failures2`, as the runner does, and are skipped. After the first failure the runner is shrinking, and calls are passed through unrecorded.
- For each recorded test case the crate stores the outcome (pass, reject, fail), the time and, with coverage, the counters and regions it ran.
- Coverage reads LLVM's counter array (through `__llvm_profile_begin_counters`), copying only the counters of functions in the code under test. The coverage mapping is read once per process from the running test binary's `__llvm_covmap` and `__llvm_covfun` sections (Mach-O or ELF, through the `object` crate), and the profile data records (`__llvm_prf_data`) locate each function's counters.
- When the runner returns, the crate computes the three numbers, appends the JSON line and, with `RESIDUAL_RISK=1`, prints the block.

## Running this crate's tests

```bash
cargo test -- --test-threads=1
RUSTFLAGS="-C instrument-coverage" cargo test --features coverage -- --test-threads=1
cargo llvm-cov --no-report --features coverage -- --test-threads=1
```

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT license](LICENSE-MIT), at your option.

## References

- Böhme, *STADS: Software Testing as Species Discovery*, TOSEM 2018.
- Böhme, Liyanage, Wüstholz, *Estimating Residual Risk in Greybox Fuzzing*, ESEC/FSE 2021.
- Lee, Böhme, *Dependency-aware Residual Risk Analysis*, ICSE 2026.
- Shi, Keles, Goldstein, Pierce, Lampropoulos, *Etna: An Evaluation Platform for Property-Based Testing*, ICFP 2023.
- Good, *The Population Frequencies of Species and the Estimation of Population Parameters*, Biometrika 1953.
