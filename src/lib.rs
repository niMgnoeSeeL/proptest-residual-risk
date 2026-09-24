//! How much a passing proptest has shown.
//!
//! Replace proptest's macro with this crate's, and nothing else changes:
//!
//! ```ignore
//! use proptest::prelude::*;
//! use proptest_residual_risk::proptest;
//! ```
//!
//! For each test, the crate records every test case the runner newly generates: whether it
//! passed, was rejected by `prop_assume!`, or failed, and how long it took. With the `coverage`
//! feature and `RUSTFLAGS="-C instrument-coverage"` it also records which LLVM coverage regions
//! of the code under test each test case ran. When a test passes, it computes from the $n$
//! passing test cases:
//!
//! 1. coverage: how many regions of the code under test the passing test cases ran;
//! 2. the chance that the next test case runs a region no test case has run,
//!    $\max(k/n,\ 1/(n+2))$, where $k$ is the number of test cases that ran a region no other
//!    test case ran (counted in LLVM counters where the regions cannot be computed);
//! 3. the 95% upper bound on the chance that the next test case fails, $1 - 0.05^{1/n}$, and how
//!    many passing test cases in total bring it below a target.
//!
//! Replays of persisted failures and rejected test cases are not counted in $n$. When a test
//! fails, nothing is estimated.
//!
//! # Switches
//!
//! All are environment variables, named `RESIDUAL_RISK*` rather than `PROPTEST_*` because
//! proptest warns about every `PROPTEST_` variable it does not know.
//!
//! - `RESIDUAL_RISK=1`: also write the numbers to the test's stdout (shown by
//!   `cargo test -- --show-output`).
//! - `RESIDUAL_RISK_TARGET`: the failure probability to compute the needed number of test cases
//!   for (default 0.001).
//! - `RESIDUAL_RISK_DIR`: where the records go (default
//!   `<target>/<profile>/proptest-residual-risk/`); one JSON line per test run is appended to
//!   `<test>.jsonl`.
//! - `RESIDUAL_RISK_TRACE=1`: also write each passing test case's counters and regions.
//! - `RESIDUAL_RISK_CODE`: the code under test, as comma-separated path prefixes; a prefix
//!   starting with `!` excludes (default: every source file except dependencies, the standard
//!   library, `tests/`, `benches/`, `examples/` and this crate).
//!
//! Measure coverage in the default (debug) test build and with `--test-threads=1` or
//! `cargo nextest`: an optimised build can drop counter updates, and the counters are shared by
//! the whole process.

pub mod coverage;
pub mod estimators;
#[cfg(feature = "coverage")]
pub mod mapping;

#[doc(hidden)]
pub use proptest as __proptest;

#[doc(hidden)]
pub mod __private {
    use crate::{coverage, estimators};
    use proptest::test_runner::{Config, TestCaseError, TestCaseResult, TestRunner};
    use std::cell::RefCell;
    use std::fmt::Write as _;
    use std::io::Write as _;
    use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
    use std::path::PathBuf;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Outcome {
        Pass,
        Reject,
        Fail,
    }

    struct Case {
        outcome: Outcome,
        time: Duration,
        /// Whether coverage was taken for this test case (every `coverage_every()`-th one).
        measured: bool,
        counters: Vec<u32>,
        regions: Vec<u32>,
    }

    #[derive(Default)]
    struct State {
        calls: usize,
        failed: bool,
        cases: Vec<Case>,
    }

    /// One run of one test function.
    pub struct Campaign {
        test: String,
        // Calls of the test body that replay persisted failures; they come first and are not
        // draws from the strategy, so they are not recorded.
        replays: usize,
        state: RefCell<State>,
    }

    impl Campaign {
        pub fn new(config: &Config, fallback_name: &str) -> Self {
            let replays = config
                .failure_persistence
                .as_ref()
                .map(|f| f.load_persisted_failures2(config.source_file).len())
                .unwrap_or(0);
            Campaign {
                test: config.test_name.unwrap_or(fallback_name).to_string(),
                replays,
                state: RefCell::new(State::default()),
            }
        }

        /// Runs one test case's body and records it. After the first failing test case the
        /// runner is shrinking: those calls are passed through unrecorded.
        pub fn observe(&self, body: impl FnOnce() -> TestCaseResult) -> TestCaseResult {
            let skip = {
                let mut st = self.state.borrow_mut();
                st.calls += 1;
                st.calls <= self.replays || st.failed
            };
            if skip {
                return body();
            }
            let measured = self.state.borrow().cases.len() % coverage_every() == 0;
            let start = Instant::now();
            let (result, counters, regions) = if measured {
                let snapshot = coverage::begin();
                let result = catch_unwind(AssertUnwindSafe(body));
                let (counters, regions) = coverage::end(snapshot);
                (result, counters, regions)
            } else {
                (catch_unwind(AssertUnwindSafe(body)), Vec::new(), Vec::new())
            };
            let time = start.elapsed();
            let outcome = match &result {
                Ok(Ok(())) => Outcome::Pass,
                Ok(Err(TestCaseError::Reject(_))) => Outcome::Reject,
                _ => Outcome::Fail,
            };
            {
                let mut st = self.state.borrow_mut();
                st.failed |= outcome == Outcome::Fail;
                st.cases.push(Case {
                    outcome,
                    time,
                    measured,
                    counters,
                    regions,
                });
            }
            match result {
                Ok(r) => r,
                Err(panic) => resume_unwind(panic),
            }
        }

        /// Computes the numbers once the runner has returned, writes the JSON line and, when
        /// asked for, the block.
        pub fn finish(&self, runner: &TestRunner, passed: bool, wall: Duration) {
            let compute_start = Instant::now();
            let st = self.state.borrow();
            let passes: Vec<&Case> = st
                .cases
                .iter()
                .filter(|c| c.outcome == Outcome::Pass)
                .collect();
            let rejects = st
                .cases
                .iter()
                .filter(|c| c.outcome == Outcome::Reject)
                .count();
            let n = passes.len() as u64;
            let stats = format!("{}", runner);
            let successes = field(&stats, "successes:");
            let target = std::env::var("RESIDUAL_RISK_TARGET")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|t| *t > 0.0 && *t < 1.0)
                .unwrap_or(0.001);
            // The chance of new code is computed from the passing test cases whose coverage was
            // taken; they are independent draws like the rest, only fewer when
            // RESIDUAL_RISK_COVERAGE_EVERY is above 1.
            let measured: Vec<&&Case> = passes.iter().filter(|c| c.measured).collect();
            let sets: Vec<&[u32]> = measured.iter().map(|c| c.counters.as_slice()).collect();
            let new_code = estimators::new_code(&sets);
            let region_sets: Vec<&[u32]> = measured.iter().map(|c| c.regions.as_slice()).collect();
            let total_regions = coverage::total_regions();
            let new_code_regions = estimators::new_code(&region_sets);
            let regions_ok = coverage::error().is_none() && total_regions.is_some_and(|t| t > 0);
            let bound = estimators::failure_bound(n);
            let needed = estimators::cases_for_target(target);
            let t = if n > 0 {
                wall.as_secs_f64() / n as f64
            } else {
                0.0
            };
            let body_time: f64 = st.cases.iter().map(|c| c.time.as_secs_f64()).sum();
            let consistent = !passed || successes == Some(n);

            let mut json = String::new();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = write!(
                json,
                "{{\"test\":{},\"unix_time\":{},\"passed\":{},\"n\":{},\"runner_successes\":{},\
                 \"counts_agree\":{},\"rejects\":{},\"wall_s\":{:.6},\"body_s\":{:.6},\"t_s\":{:.9},\
                 \"failure_bound\":{:.9},\"target\":{},\"cases_for_target\":{},\"coverage\":{}",
                quote(&self.test),
                now,
                passed,
                n,
                successes.map_or("null".to_string(), |s| s.to_string()),
                consistent,
                rejects,
                wall.as_secs_f64(),
                body_time,
                t,
                bound,
                target,
                needed,
                coverage::ON,
            );
            if coverage::ON {
                let _ = write!(
                    json,
                    ",\"distinct_counters\":{},\"singletons\":{},\"k\":{},\"p_new\":{:.9}",
                    new_code.distinct, new_code.singletons, new_code.k, new_code.chance
                );
                if let Some(why) = coverage::error() {
                    let _ = write!(json, ",\"coverage_error\":{}", quote(&why));
                }
                let _ = write!(json, ",\"window_counters\":{}", coverage::window_size());
                let _ = write!(
                    json,
                    ",\"coverage_every\":{},\"n_coverage\":{},\"mapping_s\":{:.6}",
                    coverage_every(),
                    measured.len(),
                    coverage::index_seconds()
                );
                let _ = write!(
                    json,
                    ",\"p_new_basis\":\"{}\"",
                    if regions_ok { "regions" } else { "counters" }
                );
                if let Some(total) = total_regions {
                    let files: Vec<String> = coverage::files().iter().map(|f| quote(f)).collect();
                    let _ = write!(
                        json,
                        ",\"total_regions\":{},\"distinct_regions\":{},\"region_singletons\":{},\
                         \"k_regions\":{},\"p_new_regions\":{:.9},\"code_files\":[{}]",
                        total,
                        new_code_regions.distinct,
                        new_code_regions.singletons,
                        new_code_regions.k,
                        new_code_regions.chance,
                        files.join(",")
                    );
                }
            }
            // The time spent above, computing the numbers after the runner returned.
            let _ = write!(
                json,
                ",\"compute_s\":{:.6}",
                compute_start.elapsed().as_secs_f64()
            );
            json.push('}');
            let dir = out_dir();
            let file = sanitize(&self.test);
            append(&dir.join(format!("{file}.jsonl")), &json);
            if coverage::ON && std::env::var("RESIDUAL_RISK_TRACE").as_deref() == Ok("1") {
                let mut trace = format!(
                    "{{\"test\":{},\"unix_time\":{now},\"sets\":[",
                    quote(&self.test)
                );
                for (i, s) in sets.iter().enumerate() {
                    if i > 0 {
                        trace.push(',');
                    }
                    let _ = write!(trace, "{:?}", s);
                }
                trace.push_str("],\"region_sets\":[");
                for (i, s) in region_sets.iter().enumerate() {
                    if i > 0 {
                        trace.push(',');
                    }
                    let _ = write!(trace, "{:?}", s);
                }
                trace.push_str("]}");
                append(&dir.join(format!("{file}.trace.jsonl")), &trace);
            }

            if std::env::var("RESIDUAL_RISK").as_deref() == Ok("1") {
                let mut out = String::new();
                if !passed {
                    let _ = writeln!(out, "proptest: counterexample found, nothing to estimate");
                } else {
                    let _ = writeln!(out, "proptest: {} passing test cases", group(n));
                    out.push_str(&stats);
                    if let Some(why) = coverage::error() {
                        let _ = writeln!(out, "\tcoverage: not measured: {why}");
                    }
                    if coverage::ON {
                        match total_regions {
                            Some(total) if total > 0 => {
                                let _ = writeln!(
                                    out,
                                    "\tcoverage: {} of {} regions ({:.0}%) in the code under test, {} counters",
                                    group(new_code_regions.distinct),
                                    group(total as u64),
                                    100.0 * new_code_regions.distinct as f64 / total as f64,
                                    group(new_code.distinct)
                                );
                            }
                            _ => {
                                let _ = writeln!(
                                    out,
                                    "\tcoverage: {} counters raised",
                                    group(new_code.distinct)
                                );
                            }
                        }
                        // The chance of new code counts regions; where the
                        // regions cannot be computed it falls back to counters and says so.
                        let (p, basis) = if regions_ok {
                            (new_code_regions.chance, "")
                        } else {
                            (
                                new_code.chance,
                                " (counted in counters: regions unavailable)",
                            )
                        };
                        let _ = writeln!(
                            out,
                            "\tnew code: about {} chance per test case (next new code after ~{} more, ~{}){}",
                            sig(p),
                            group((1.0 / p).round() as u64),
                            secs(t / p),
                            basis
                        );
                        if coverage_every() > 1 {
                            let _ = writeln!(
                                out,
                                "\t          coverage taken for every {}th test case: {} of {}",
                                coverage_every(),
                                group(measured.len() as u64),
                                group(n)
                            );
                        }
                    }
                    let _ = writeln!(
                        out,
                        "\tfailure: at most {} chance per test case (95% confidence)",
                        sig(bound)
                    );
                    if bound > target {
                        let more = needed - n;
                        let _ = writeln!(
                            out,
                            "\t         below {} after {} more (~{}): PROPTEST_CASES={}",
                            target,
                            group(more),
                            secs(t * more as f64),
                            needed
                        );
                    } else {
                        let _ = writeln!(out, "\t         already below {}", target);
                    }
                    if !consistent {
                        let _ = writeln!(
                            out,
                            "\tnote: counted {} passing test cases, the runner counted {:?}",
                            n, successes
                        );
                    }
                }
                print!("{out}");
            }
        }
    }

    fn field(stats: &str, name: &str) -> Option<u64> {
        stats
            .lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .and_then(|v| v.trim().parse().ok())
    }

    /// Coverage is taken for every k-th test case the runner generates, k from
    /// RESIDUAL_RISK_COVERAGE_EVERY (default 1: every test case). Read once per process.
    fn coverage_every() -> usize {
        static EVERY: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        *EVERY.get_or_init(|| {
            std::env::var("RESIDUAL_RISK_COVERAGE_EVERY")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|k| *k >= 1)
                .unwrap_or(1)
        })
    }

    fn out_dir() -> PathBuf {
        if let Some(d) = std::env::var_os("RESIDUAL_RISK_DIR") {
            return PathBuf::from(d);
        }
        // A test binary lives in <target>/<profile>/deps/.
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent()?.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(std::env::temp_dir)
            .join("proptest-residual-risk")
    }

    fn append(path: &std::path::Path, line: &str) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }

    fn sanitize(name: &str) -> String {
        name.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn quote(s: &str) -> String {
        let mut q = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => q.push_str("\\\""),
                '\\' => q.push_str("\\\\"),
                c if (c as u32) < 0x20 => {
                    let _ = write!(q, "\\u{:04x}", c as u32);
                }
                c => q.push(c),
            }
        }
        q.push('"');
        q
    }

    // Two significant digits, written as a decimal: 0.012, 0.0039, 0.00012.
    fn sig(x: f64) -> String {
        if x <= 0.0 || x >= 1.0 {
            return format!("{x}");
        }
        let decimals = (-x.log10()).floor() as usize + 2;
        format!("{:.*}", decimals, x)
    }

    fn secs(s: f64) -> String {
        if s < 1.0 {
            format!("{:.3} s", s)
        } else {
            format!("{:.1} s", s)
        }
    }

    fn group(n: u64) -> String {
        let s = n.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        out
    }
}

/// The body of one test: proptest's `@_BODY` / `@_BODY2` with the test body observed.
#[doc(hidden)]
#[macro_export]
macro_rules! __rr_body {
    (@_BODY $config:ident ($($parm:pat in $strategy:expr),+) [$($mod:tt)*] $body:expr) => {{
        $config.source_file = ::core::option::Option::Some(::core::file!());
        let mut runner = $crate::__proptest::test_runner::TestRunner::new($config);
        let campaign_value = $crate::__private::Campaign::new(
            runner.config(), ::core::concat!(::core::file!(), ":", ::core::line!()));
        let campaign = &campaign_value;
        let names = $crate::__proptest::proptest_helper!(@_WRAPSTR ($($parm),*));
        let start = ::std::time::Instant::now();
        let result = runner.run(
            &$crate::__proptest::strategy::Strategy::prop_map(
                $crate::__proptest::proptest_helper!(@_WRAP ($($strategy)*)),
                |values| $crate::__proptest::sugar::NamedArguments(names, values)),
            $($mod)* |$crate::__proptest::sugar::NamedArguments(
                _, $crate::__proptest::proptest_helper!(@_WRAPPAT ($($parm),*)))|
            {
                campaign.observe(|| {
                    let (): () = $body;
                    ::core::result::Result::Ok(())
                })
            });
        campaign.finish(&runner, result.is_ok(), start.elapsed());
        match result {
            ::core::result::Result::Ok(()) => (),
            ::core::result::Result::Err(e) => ::core::panic!("{}\n{}", e, runner),
        }
    }};
    (@_BODY2 $config:ident ($($arg:tt)+) [$($mod:tt)*] $body:expr) => {{
        $config.source_file = ::core::option::Option::Some(::core::file!());
        let mut runner = $crate::__proptest::test_runner::TestRunner::new($config);
        let campaign_value = $crate::__private::Campaign::new(
            runner.config(), ::core::concat!(::core::file!(), ":", ::core::line!()));
        let campaign = &campaign_value;
        let names = $crate::__proptest::proptest_helper!(@_EXT _STR ($($arg)*));
        let start = ::std::time::Instant::now();
        let result = runner.run(
            &$crate::__proptest::strategy::Strategy::prop_map(
                $crate::__proptest::proptest_helper!(@_EXT _STRAT ($($arg)*)),
                |values| $crate::__proptest::sugar::NamedArguments(names, values)),
            $($mod)* |$crate::__proptest::sugar::NamedArguments(
                _, $crate::__proptest::proptest_helper!(@_EXT _PAT ($($arg)*)))|
            {
                campaign.observe(|| {
                    let (): () = $body;
                    ::core::result::Result::Ok(())
                })
            });
        campaign.finish(&runner, result.is_ok(), start.elapsed());
        match result {
            ::core::result::Result::Ok(()) => (),
            ::core::result::Result::Err(e) => ::core::panic!("{}\n{}", e, runner),
        }
    }};
}

/// proptest's `proptest!` with the same twelve forms; each test's body is observed.
#[macro_export]
macro_rules! proptest {
    (#![proptest_config($config:expr)]
     $(
        $(#[$meta:meta])*
       fn $test_name:ident($($parm:pat in $strategy:expr),+ $(,)?) $body:block
    )*) => {
        $(
            $(#[$meta])*
            fn $test_name() {
                let mut config = $crate::__proptest::test_runner::contextualize_config($config.clone());
                config.test_name = ::core::option::Option::Some(
                    ::core::concat!(::core::module_path!(), "::", ::core::stringify!($test_name)));
                $crate::__rr_body!(@_BODY config ($($parm in $strategy),+) [] $body);
            }
        )*
    };
    (#![proptest_config($config:expr)]
     $(
        $(#[$meta:meta])*
        fn $test_name:ident($($arg:tt)+) $body:block
    )*) => {
        $(
            $(#[$meta])*
            fn $test_name() {
                let mut config = $crate::__proptest::test_runner::contextualize_config($config.clone());
                config.test_name = ::core::option::Option::Some(
                    ::core::concat!(::core::module_path!(), "::", ::core::stringify!($test_name)));
                $crate::__rr_body!(@_BODY2 config ($($arg)+) [] $body);
            }
        )*
    };

    ($(
        $(#[$meta:meta])*
        fn $test_name:ident($($parm:pat in $strategy:expr),+ $(,)?) $body:block
    )*) => { $crate::proptest! {
        #![proptest_config($crate::__proptest::test_runner::Config::default())]
        $($(#[$meta])*
          fn $test_name($($parm in $strategy),+) $body)*
    } };

    ($(
        $(#[$meta:meta])*
        fn $test_name:ident($($arg:tt)+) $body:block
    )*) => { $crate::proptest! {
        #![proptest_config($crate::__proptest::test_runner::Config::default())]
        $($(#[$meta])*
          fn $test_name($($arg)+) $body)*
    } };

    (|($($parm:pat in $strategy:expr),+ $(,)?)| $body:expr) => {
        $crate::proptest!(
            $crate::__proptest::test_runner::Config::default(),
            |($($parm in $strategy),+)| $body)
    };

    (move |($($parm:pat in $strategy:expr),+ $(,)?)| $body:expr) => {
        $crate::proptest!(
            $crate::__proptest::test_runner::Config::default(),
            move |($($parm in $strategy),+)| $body)
    };

    (|($($arg:tt)+)| $body:expr) => {
        $crate::proptest!(
            $crate::__proptest::test_runner::Config::default(),
            |($($arg)+)| $body)
    };

    (move |($($arg:tt)+)| $body:expr) => {
        $crate::proptest!(
            $crate::__proptest::test_runner::Config::default(),
            move |($($arg)+)| $body)
    };

    ($config:expr, |($($parm:pat in $strategy:expr),+ $(,)?)| $body:expr) => { {
        let mut config = $crate::__proptest::test_runner::contextualize_config($config.__sugar_to_owned());
        $crate::__proptest::sugar::force_no_fork(&mut config);
        $crate::__rr_body!(@_BODY config ($($parm in $strategy),+) [] $body)
    } };

    ($config:expr, move |($($parm:pat in $strategy:expr),+ $(,)?)| $body:expr) => { {
        let mut config = $crate::__proptest::test_runner::contextualize_config($config.__sugar_to_owned());
        $crate::__proptest::sugar::force_no_fork(&mut config);
        $crate::__rr_body!(@_BODY config ($($parm in $strategy),+) [move] $body)
    } };

    ($config:expr, |($($arg:tt)+)| $body:expr) => { {
        let mut config = $crate::__proptest::test_runner::contextualize_config($config.__sugar_to_owned());
        $crate::__proptest::sugar::force_no_fork(&mut config);
        $crate::__rr_body!(@_BODY2 config ($($arg)+) [] $body);
    } };

    ($config:expr, move |($($arg:tt)+)| $body:expr) => { {
        let mut config = $crate::__proptest::test_runner::contextualize_config($config.__sugar_to_owned());
        $crate::__proptest::sugar::force_no_fork(&mut config);
        $crate::__rr_body!(@_BODY2 config ($($arg)+) [move] $body);
    } };
}
