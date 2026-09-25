# Theory

[← README](../README.md) · **Theory** · [Evidence](evidence.md) · [Reference](reference.md)

This page defines the three numbers the crate reports, and states what they assume.

- [Which test cases are counted](#which-test-cases-are-counted)
- [Failure bound](#failure-bound)
- [Chance of new code](#chance-of-new-code)
- [Coverage](#coverage)
- [Assumptions and caveats](#assumptions-and-caveats)
- [References](#references)

## Which test cases are counted

All three numbers are computed from the test cases that proptest's runner newly generated and that passed. We write $n$ for their number. $n$ equals proptest's own `successes`, whether or not the test passed; the record file says whether the two agree (`counts_agree`).

Not counted:

- replays of persisted failures (`proptest-regressions` files), which the runner runs before any new test case;
- test cases rejected by `prop_assume!`, and values redrawn by `prop_filter`;
- shrinking, which only happens after a failure, including the failure of a replayed seed. When a test fails, nothing is estimated.

## Failure bound

If $n$ test cases, drawn independently from the same distribution, all passed, then

$$\bar p_{95} = 1 - 0.05^{1/n} \approx \frac{3}{n}$$

is the exact binomial 95% upper confidence bound on the probability that one more test case from that distribution fails. With proptest's default of 256 cases it is 0.0116.

To bring the bound below a target $\varepsilon$, the number of passing test cases needed in total is

$$N_\varepsilon = \left\lceil \frac{\ln 0.05}{\ln(1-\varepsilon)} \right\rceil .$$

For $\varepsilon = 0.001$ (the default target) that is 2,995.

| Passing test cases $n$ | 100 | 256 | 1,000 | 2,995 | 10,000 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Bound $1 - 0.05^{1/n}$ | 0.0295 | 0.0116 | 0.0030 | 0.0010 | 0.0003 |

## Chance of new code

Call a coverage element (a region; see [Coverage](#coverage)) a *singleton* if exactly one of the $n$ passing test cases ran it. Let $k$ be the number of test cases that ran at least one singleton. The estimate of the chance that the next test case runs an element that no test case has run is

$$\hat p_{\text{new}} = \max\left(\frac{k}{n},\ \frac{1}{n+2}\right).$$

- $k/n$ is Ma and Chao's estimator of the chance of seeing a new species, as adapted to code coverage by Lee and Böhme (ICSE 2026). One of the authors of that paper is behind this crate.
- $1/(n+2)$ is Laplace's rule of succession. It keeps the estimate from reading 0 after finitely many test cases; with $n = 256$ it is $1/258 = 0.0039$.
- The expected number of further test cases until new code runs is $1/\hat p_{\text{new}}$.

**Example.** Three test cases ran the regions $\{a, b, c\}$, $\{a, b\}$ and $\{a, d, e\}$:

| Test case | Regions it ran | Regions only it ran |
| --- | --- | --- |
| 1 | a, b, c | c |
| 2 | a, b | – |
| 3 | a, d, e | d, e |

Test cases 1 and 3 ran a singleton, so $k = 2$ and $\hat p_{\text{new}} = \max(2/3,\ 1/5) = 2/3$.

## Coverage

Coverage is counted in LLVM **regions**: source ranges that the compiler tracks under `-C instrument-coverage`. The compiler also stores, in the test binary, a *coverage mapping*: for each function, its regions and how each region's execution count follows from LLVM's **counters**. The crate reads the counters before and after each test case, and a region ran in a test case when its count went up.

**Why regions and not counters.** LLVM gives no counter of its own to code whose count it can compute from other counters:

```rust
fn clamp(x: i64, lo: i64, hi: i64) -> i64 {   // counter A: entry
    if x < lo { return lo; }                   // counter B
    if x > hi { return hi; }                   // counter C
    x                                          // no counter: count = A - B - C
}
```

| Test case | Counters raised | New by counters | New by regions |
| --- | --- | --- | --- |
| `clamp(-5, 0, 9)` | A, B | A, B | entry, `return lo` |
| `clamp(50, 0, 9)` | A, C | C | `return hi` |
| `clamp(5, 0, 9)` | A | **none** | **`x`** |

The third test case runs `x` for the first time, but raises only counter A, which was already seen. Counted in counters, it would not count as new code. On 57 real crates, counting regions found 14% more test cases that ran new code ([evidence](evidence.md#the-chance-of-new-code)).

**The code under test.** Coverage and the chance of new code count only regions in the code under test. By default it is every source file except dependencies (`~/.cargo/registry`, `~/.cargo/git`), the standard library, files under `tests/`, `benches/` and `examples/`, and this crate's own `src/`. `RESIDUAL_RISK_CODE` sets it explicitly ([reference](reference.md#configuration)).

## Assumptions and caveats

### Independent test cases (the failure bound and the chance of new code)

The failure bound and the chance of new code both treat the $n$ passing test cases as independent draws from one distribution. proptest meets this. Every test case starts from a fresh seed: `TestRunner::run_in_process_with_replay` calls `gen_get_seed` and then the strategy's `new_tree`. proptest does not mutate earlier inputs, avoid earlier inputs, or steer generation by coverage. So the passing test cases are independent draws from the strategy's distribution, restricted to inputs that pass `prop_assume!` and `prop_filter`.

A change that makes early test cases differ from later ones would break this, for example the edge-bias proposal in proptest #515, which would favour boundary values early in a run. Replays of persisted failures are excluded for the same reason ([which test cases are counted](#which-test-cases-are-counted)).

### The distribution is the strategy's

All probabilities are over the test's strategy, not over the inputs your code meets in production. If the strategy never generates some input, no number here says anything about it. The failure bound is not the chance that your code has a bug; it is the chance that one more test case from this strategy fails.

### The chance of new code is an estimate, not a bound

It is right on average, but in a single run it can be below the truth. On 57 real crates it was below the true rate by more than sampling noise in 18% of the campaigns where it was above its floor ([evidence](evidence.md#the-chance-of-new-code)). This is why the output says "about". The failure bound, by contrast, is a bound: it is below the truth in at most 5% of passing tests ([evidence](evidence.md#the-failure-bound)).

### The chance of new code does not bound the chance of failure

The chance that the next test case fails can be higher than the chance that it runs new code. Here the defect is in a branch no passing test case ran, yet the chance of new code reads lower than the true failure probability:

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

| Quantity | Value in a passing run (`PROPTEST_RNG_SEED=12`, proptest 1.11.0) | Compared with the true 0.0099 |
| --- | ---: | --- |
| True failure probability, $1/101$ | 0.0099 | |
| Chance of new code | 0.0039 | below |
| Failure bound | 0.0116 | above |

No passing test case ran the `else` branch, and no region was a singleton, so the chance of new code sat at its floor. Use the failure bound for "how likely is a failure", and the chance of new code for "is my strategy still finding new behaviour".

### Coverage needs a debug build and one test at a time

In an optimised build LLVM can merge or drop the counter updates of inlined code; in one experiment a call of `clamp` with constant arguments raised no counter at all. LLVM's counters are shared by the whole process, so tests running at the same time would mix their coverage: run with `--test-threads=1` or with `cargo nextest`.

## References

- I. J. Good, *The Population Frequencies of Species and the Estimation of Population Parameters*, Biometrika 1953.
- M. Böhme, *STADS: Software Testing as Species Discovery*, TOSEM 2018.
- M. Böhme, D. Liyanage, V. Wüstholz, *Estimating Residual Risk in Greybox Fuzzing*, ESEC/FSE 2021.
- S. Lee, M. Böhme, *Dependency-aware Residual Risk Analysis*, ICSE 2026.
