//! The code under test for the README's recording.

/// `x` limited to the range from `lo` to `hi`.
pub fn clamp(x: i64, lo: i64, hi: i64) -> i64 {
    if x < lo {
        return lo;
    }
    if x > hi {
        return hi;
    }
    x
}

/// The mean of two numbers, rounded down.
pub fn mean(a: u8, b: u8) -> u8 {
    ((a as u16 + b as u16) / 2) as u8
}

#[cfg(test)]
mod tests {
    use super::clamp;
    use proptest::prelude::*;
    use proptest_residual_risk::proptest;

    proptest! {
        #[test]
        fn clamp_in_range(x: i64, lo: i64, hi: i64) {
            prop_assume!(lo <= hi);
            let y = clamp(x, lo, hi);
            prop_assert!(lo <= y && y <= hi);
        }
    }
}
