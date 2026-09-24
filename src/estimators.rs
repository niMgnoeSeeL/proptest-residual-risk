//! The three numbers, computed from the passing test cases of one campaign.
//!
//! Every function here is pure, so it is tested on its own.

/// The exact binomial 95% upper confidence bound on the failure probability after `n`
/// independent passing test cases: $1 - 0.05^{1/n}$. With `n = 0` nothing is known and the
/// bound is 1.
pub fn failure_bound(n: u64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    1.0 - 0.05f64.powf(1.0 / n as f64)
}

/// The total number of passing test cases needed for [`failure_bound`] to be at most `target`:
/// $\lceil \ln 0.05 / \ln(1 - \varepsilon) \rceil$.
pub fn cases_for_target(target: f64) -> u64 {
    assert!(target > 0.0 && target < 1.0, "target must be in (0, 1)");
    let exact = 0.05f64.ln() / (1.0 - target).ln();
    let mut n = exact.ceil() as u64;
    // Guard against rounding: the bound at n must really be at most the target.
    while failure_bound(n) > target {
        n += 1;
    }
    n
}

/// What the chance of new code is computed from.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCode {
    /// Test cases that ran at least one coverage element no other test case ran ($\lvert Y_D\rvert$).
    pub k: u64,
    /// Coverage elements run by exactly one test case.
    pub singletons: u64,
    /// Distinct coverage elements run by any test case.
    pub distinct: u64,
    /// $\max(k/n,\ 1/(n+2))$.
    pub chance: f64,
}

/// The chance that the next test case runs a coverage element that no test case has run:
/// $\max(\lvert Y_D\rvert/n,\ 1/(n+2))$ (Ma and Chao's estimator as adapted in Lee and Böhme,
/// ICSE 2026, with Laplace's rule of succession as the floor). `sets` holds, for each passing
/// test case, the coverage elements it ran.
///
/// Elements are small integers (counter indices or region ids), so they are counted in an array
/// indexed by the element rather than a hash map: in an unoptimised build this is what keeps the
/// computation after a campaign short.
pub fn new_code<S: AsRef<[u32]>>(sets: &[S]) -> NewCode {
    let n = sets.len() as u64;
    let size = sets
        .iter()
        .flat_map(|s| s.as_ref().iter())
        .copied()
        .max()
        .map_or(0, |m| m as usize + 1);
    let mut runs = vec![0u32; size];
    for set in sets {
        for &e in set.as_ref() {
            runs[e as usize] += 1;
        }
    }
    let k = sets
        .iter()
        .filter(|set| set.as_ref().iter().any(|&e| runs[e as usize] == 1))
        .count() as u64;
    let singletons = runs.iter().filter(|&&c| c == 1).count() as u64;
    let distinct = runs.iter().filter(|&&c| c > 0).count() as u64;
    let floor = 1.0 / (n as f64 + 2.0);
    let chance = if n == 0 {
        1.0
    } else {
        (k as f64 / n as f64).max(floor)
    };
    NewCode {
        k,
        singletons,
        distinct,
        chance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_matches_the_closed_form() {
        assert!((failure_bound(100) - 0.029513).abs() < 1e-6);
        assert!((failure_bound(256) - 0.011634).abs() < 1e-6);
        assert_eq!(failure_bound(0), 1.0);
    }

    #[test]
    fn cases_for_target_is_the_smallest_n() {
        let n = cases_for_target(0.001);
        assert_eq!(n, 2995);
        assert!(failure_bound(n) <= 0.001);
        assert!(failure_bound(n - 1) > 0.001);
    }

    #[test]
    fn worked_example_of_the_readme() {
        // Three test cases: X1 = {a,b,c}, X2 = {a,b}, X3 = {a,d,e}.
        let (a, b, c, d, e) = (0, 1, 2, 3, 4);
        let r = new_code(&[vec![a, b, c], vec![a, b], vec![a, d, e]]);
        assert_eq!(r.k, 2);
        assert_eq!(r.singletons, 3);
        assert_eq!(r.distinct, 5);
        assert!((r.chance - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn no_singleton_gives_the_laplace_floor() {
        let sets = vec![vec![7, 8]; 256];
        let r = new_code(&sets);
        assert_eq!(r.k, 0);
        assert!((r.chance - 1.0 / 258.0).abs() < 1e-12);
    }
}
