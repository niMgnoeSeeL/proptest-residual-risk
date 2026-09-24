// Every written form of proptest! compiles and runs through this crate's macro.
use proptest::prelude::*;
use proptest_residual_risk::proptest;

proptest! {
    #[test]
    fn form_in(x in 0u8..10, y in any::<bool>()) {
        prop_assert!(x < 10 || y);
    }

    #[test]
    fn form_type(x: u8, _y: bool) {
        prop_assert!(x as u16 <= 255);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(17))]
    #[test]
    fn form_config_in(x in 0u32..5) {
        prop_assert!(x < 5);
    }

    #[test]
    fn form_config_type(x: u16) {
        prop_assert!(x as u32 <= 65535);
    }
}

#[test]
fn form_closures() {
    proptest!(|(x in 0u8..3)| { prop_assert!(x < 3); });
    let bound = 4u8;
    proptest!(move |(x in 0u8..4)| { prop_assert!(x < bound); });
    proptest!(|(x: u8)| { prop_assert!(x as u16 <= 255); });
    proptest!(move |(x: u8)| { prop_assert!(x as u16 <= 255 + bound as u16); });
    proptest!(ProptestConfig::with_cases(5), |(x in 0u8..3)| { prop_assert!(x < 3); });
    proptest!(ProptestConfig::with_cases(5), move |(x in 0u8..3)| { prop_assert!(x < bound); });
    proptest!(ProptestConfig::with_cases(5), |(x: u8)| { prop_assert!(x as u16 <= 255); });
    proptest!(ProptestConfig::with_cases(5), move |(x: u8)| { prop_assert!(x as u16 <= 255 + bound as u16); });
}
