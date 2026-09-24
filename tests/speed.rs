// Time of the computation after a campaign, on sets shaped like a real crate's (toml_edit in the
// new-code measurement: 5,256 passing test cases, about 229 regions each). Run with
// `cargo test --test speed -- --ignored --nocapture`.
use proptest_residual_risk::estimators::new_code;
use std::time::Instant;

fn sets() -> Vec<Vec<u32>> {
    let mut x: u64 = 12345;
    let mut next = || {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 33) as u32
    };
    (0..5256)
        .map(|_| {
            // 200 regions every test case runs, and 29 drawn from 3,000 rarer ones.
            let mut s: Vec<u32> = (0..200).collect();
            s.extend((0..29).map(|_| 200 + next() % 3000));
            s.sort_unstable();
            s.dedup();
            s
        })
        .collect()
}

#[test]
#[ignore]
fn time_new_code() {
    let s = sets();
    let t = Instant::now();
    let r = new_code(&s);
    let one = t.elapsed();
    println!(
        "new_code on {} sets: {:?} (k = {}, distinct = {})",
        s.len(),
        one,
        r.k,
        r.distinct
    );
}
