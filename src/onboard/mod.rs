//! HID++ 2.0 onboard profiles (feature 0x8100).

mod feature;
pub mod format;
pub mod label;

pub use feature::{Mode, OnboardError, OnboardProfilesFeature};
