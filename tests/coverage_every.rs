// RESIDUAL_RISK_COVERAGE_EVERY: coverage for every k-th test case only. Its own test binary,
// because the crate reads the variable once per process.
#![cfg(feature = "coverage")]
mod common;

use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};
use proptest_residual_risk::proptest;

fn clamp(x: i64, lo: i64, hi: i64) -> i64 {
    if x < lo {
        return lo;
    }
    if x > hi {
        return hi;
    }
    x
}

proptest! {
    #![proptest_config(Config { rng_seed: RngSeed::Fixed(1), failure_persistence: None, ..Config::default() })]
    fn clamp_in_range(x: i64, lo: i64 , hi: i64) {
        prop_assume!(lo <= hi);
        let y = clamp(x, lo, hi);
        prop_assert!(lo <= y && y <= hi);
    }
}

#[test]
fn every_fourth() {
    std::env::set_var(
        "RESIDUAL_RISK_CODE",
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/coverage_every.rs"),
    );
    std::env::set_var("RESIDUAL_RISK_COVERAGE_EVERY", "4");
    clamp_in_range();
    let line = common::last_line("coverage_every::clamp_in_range");
    // The failure bound still counts every passing test case.
    assert_eq!(common::num(&line, "n"), 256.0, "{line}");
    assert_eq!(common::num(&line, "coverage_every"), 4.0, "{line}");
    // Every 4th generated test case (rejected ones included) is measured, so about a quarter of
    // the passing ones.
    let m = common::num(&line, "n_coverage");
    assert!((40.0..=90.0).contains(&m), "{line}");
    assert!(common::num(&line, "distinct_regions") >= 4.0, "{line}");
}
