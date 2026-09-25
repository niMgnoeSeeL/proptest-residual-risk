// With `fork`, the test cases run in child processes that this process never sees: n comes from
// the runner, so the failure bound is still right, and coverage is reported as not measured.
mod common;

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_residual_risk::proptest;

proptest! {
    #![proptest_config(Config { fork: true, cases: 20, failure_persistence: None, ..Config::default() })]
    #[test]
    fn forked(x in 0u32..1000) {
        prop_assert!(x < 1000);
    }
}

#[test]
fn fork_mode_uses_the_runner_count() {
    forked();
    let line = common::last_line("fork::forked");
    assert_eq!(common::num(&line, "n"), 20.0, "{line}");
    assert_eq!(common::num(&line, "runner_successes"), 20.0, "{line}");
    assert!(common::flag(&line, "fork"), "{line}");
    assert!(line.contains("\"counts_agree\":null"), "{line}");
    assert!((common::num(&line, "failure_bound") - (1.0 - 0.05f64.powf(1.0 / 20.0))).abs() < 1e-8);
    if cfg!(feature = "coverage") {
        assert!(line.contains("fork mode"), "{line}");
        assert!(!line.contains("\"distinct_regions\""), "{line}");
    }
}
