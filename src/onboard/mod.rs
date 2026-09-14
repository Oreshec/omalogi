//! HID++ 2.0 onboard profiles (feature 0x8100).

pub mod action;
pub mod edit;
mod feature;
pub mod format;
pub mod label;

pub use feature::{Mode, OnboardError, OnboardProfilesFeature};
