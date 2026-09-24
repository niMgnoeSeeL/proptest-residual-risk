// With the coverage feature and -C instrument-coverage: clamp's three branches.
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

// clamp lives in this test file, which the default rule leaves out of the code under test. The
// crate reads RESIDUAL_RISK_CODE once per process, so every test sets it before anything runs.
fn code_is_this_file() {
    std::env::set_var(
        "RESIDUAL_RISK_CODE",
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/coverage.rs"),
    );
}

proptest! {
    #![proptest_config(Config { rng_seed: RngSeed::Fixed(1), failure_persistence: None, ..Config::default() })]
    fn clamp_in_range(x: i64, lo: i64, hi: i64) {
        prop_assume!(lo <= hi);
        let y = clamp(x, lo, hi);
        prop_assert!(lo <= y && y <= hi);
    }
}

#[test]
fn clamp_numbers() {
    code_is_this_file();
    clamp_in_range();
    let line = common::last_line("coverage::clamp_in_range");
    assert_eq!(common::num(&line, "n"), 256.0, "{line}");
    // Every branch is run by many test cases, so no counter is a singleton.
    assert_eq!(common::num(&line, "k"), 0.0, "{line}");
    assert!(
        (common::num(&line, "p_new") - 1.0 / 258.0).abs() < 1e-9,
        "{line}"
    );
    assert!(common::num(&line, "distinct_counters") >= 3.0, "{line}");
    // Regions: every region of this file's functions that ran; clamp's three branches included.
    assert!(common::num(&line, "distinct_regions") >= 4.0, "{line}");
    assert_eq!(common::num(&line, "k_regions"), 0.0, "{line}");
}

#[test]
fn inside_is_new_only_in_regions() {
    code_is_this_file();
    use proptest_residual_risk::coverage;
    use std::hint::black_box as bb;
    let run = |x: i64| {
        let s = coverage::begin();
        bb(clamp(bb(x), 0, 9));
        coverage::end(s)
    };
    let t0 = std::time::Instant::now();
    let (below_c, below_r) = run(-5);
    let first = t0.elapsed();
    let t1 = std::time::Instant::now();
    let (above_c, above_r) = run(50);
    let (inside_c, inside_r) = run(5);
    let later = t1.elapsed() / 2;
    let seen_c: std::collections::HashSet<_> = below_c.iter().chain(&above_c).collect();
    let seen_r: std::collections::HashSet<_> = below_r.iter().chain(&above_r).collect();
    let new_c: Vec<_> = inside_c.iter().filter(|c| !seen_c.contains(c)).collect();
    let new_r: Vec<_> = inside_r.iter().filter(|r| !seen_r.contains(r)).collect();
    println!("below  counters {below_c:?} regions {below_r:?}");
    println!("above  counters {above_c:?} regions {above_r:?}");
    println!("inside counters {inside_c:?} regions {inside_r:?}");
    println!("new in inside: counters {new_c:?}, regions {new_r:?}");
    println!("first call (reads the mapping) {first:?}, later calls {later:?} each");
    assert!(new_c.is_empty(), "inside raised a counter of its own");
    assert!(!new_r.is_empty(), "inside ran no region of its own");
}
