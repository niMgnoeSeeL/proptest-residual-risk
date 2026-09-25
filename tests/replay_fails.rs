// A persisted failure that still fails on replay: the runner shrinks it right away. Those shrink
// calls are not new test cases and must not be counted in n.
mod common;

use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence};
use proptest_residual_risk::proptest;

fn persisted_file() -> &'static str {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/replay-fails.txt");
    std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target")).unwrap();
    std::fs::write(path, format!("cc {}\n", "01".repeat(32))).unwrap();
    path
}

proptest! {
    #![proptest_config(Config {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(persisted_file()))),
        ..Config::default()
    })]
    // The persisted seed draws x = 125, which fails again; the runner then shrinks it, and its
    // first shrink calls (x = 62, then 93) pass.
    fn below_100(x in 0u32..1000) {
        prop_assert!(x < 100);
    }
}

#[test]
fn shrinking_a_failing_replay_is_not_counted() {
    assert!(std::panic::catch_unwind(below_100).is_err());
    let line = common::last_line("replay_fails::below_100");
    assert!(!common::flag(&line, "passed"), "{line}");
    assert_eq!(common::num(&line, "n"), 0.0, "{line}");
    assert!(common::flag(&line, "counts_agree"), "{line}");
}

proptest! {
    #![proptest_config(Config {
        failure_persistence: None,
        rng_seed: proptest::test_runner::RngSeed::Fixed(1),
        ..Config::default()
    })]
    // No persisted seed: some new test cases pass, then one fails and is shrunk.
    fn below_900(x in 0u32..1000) {
        prop_assert!(x < 900);
    }
}

#[test]
fn a_failing_test_counts_what_the_runner_counts() {
    assert!(std::panic::catch_unwind(below_900).is_err());
    let line = common::last_line("replay_fails::below_900");
    assert!(!common::flag(&line, "passed"), "{line}");
    assert!(common::num(&line, "n") > 0.0, "{line}");
    assert_eq!(
        common::num(&line, "n"),
        common::num(&line, "runner_successes"),
        "{line}"
    );
    assert!(common::flag(&line, "counts_agree"), "{line}");
}
