//! Omalogi: Logitech G-series configuration for Omarchy.

pub mod daemon;
pub mod device;
pub mod hidraw;
pub mod hyprland;
pub mod onboard;
pub mod rules;

/// An error and its causes on one line: `outer: cause: root cause`.
#[must_use]
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
