// n is the number of newly generated passing test cases: replays of persisted failures and
// rejected test cases are not counted, and n agrees with the runner's own count.
mod common;

use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence};
use proptest_residual_risk::proptest;
use std::sync::atomic::{AtomicUsize, Ordering};

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn persisted_file() -> &'static str {
    // Three persisted failure seeds that now pass: the runner replays them first.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/counting-replays.txt");
    let seed = |b: u8| format!("cc {}\n", format!("{b:02x}").repeat(32));
    std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target")).unwrap();
    std::fs::write(path, seed(1) + &seed(2) + &seed(3)).unwrap();
    path
}

proptest! {
    #![proptest_config(Config {
        cases: 40,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(persisted_file()))),
        ..Config::default()
    })]
    fn counts_replays_and_rejects(x in 0u32..100) {
        CALLS.fetch_add(1, Ordering::SeqCst);
        prop_assume!(x % 3 != 0);
        prop_assert!(x < 100);
    }
}

#[test]
fn check_counts() {
    counts_replays_and_rejects();
    let line = common::last_line("counting::counts_replays_and_rejects");
    let n = common::num(&line, "n") as usize;
    let rejects = common::num(&line, "rejects") as usize;
    assert_eq!(n, 40, "{line}");
    assert!(common::flag(&line, "counts_agree"), "{line}");
    // Every call is a replay, a recorded pass or a recorded rejection.
    assert_eq!(CALLS.load(Ordering::SeqCst), 3 + n + rejects, "{line}");
    let bound = common::num(&line, "failure_bound");
    assert!((bound - (1.0 - 0.05f64.powf(1.0 / 40.0))).abs() < 1e-8);
}

#[test]
fn failing_test_still_panics_with_the_runner_block() {
    let r = std::panic::catch_unwind(|| {
        // A fixed seed that meets a == 7: with a random seed, 256 draws miss it with
        // probability (100/101)^256 = 0.078 and the test would not fail.
        proptest!(Config { failure_persistence: None, rng_seed: proptest::test_runner::RngSeed::Fixed(1), ..Config::default() }, |(a in 0u8..=100)| {
            prop_assert!(a != 7);
        });
    });
    let msg = r.unwrap_err();
    let text = msg.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(text.contains("minimal failing input: a = 7"), "{text}");
    assert!(text.contains("successes:"), "{text}");
}
