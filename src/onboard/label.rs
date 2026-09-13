//! Human-readable names for bindings.

use super::format::{Binding, SpecialAction};

const MOUSE_BUTTONS: [&str; 5] = [
    "left click",
    "right click",
    "middle click",
    "back",
    "forward",
];
const MODIFIERS: [&str; 8] = [
    "Ctrl",
    "Shift",
    "Alt",
    "Super",
    "Right Ctrl",
    "Right Shift",
    "Right Alt",
    "Right Super",
];

#[must_use]
pub fn binding(binding: &Binding) -> String {
    match *binding {
        Binding::Mouse { buttons } => mouse_buttons(buttons),
        Binding::Key { modifiers, key } => {
            let mut parts: Vec<String> = MODIFIERS
                .iter()
                .enumerate()
                .filter(|&(bit, _)| modifiers & (1 << bit) != 0)
                .map(|(_, name)| (*name).to_owned())
                .collect();
            parts.push(key_name(key));
            parts.join("+")
        }
        Binding::Consumer { usage } => consumer_name(usage),
        Binding::Special {
            action: Some(action),
            ..
        } => special_name(action).to_owned(),
        Binding::Special {
            code, action: None, ..
        } => format!("firmware action 0x{code:02X}"),
        Binding::Macro { .. } => "macro".to_owned(),
        Binding::Disabled => "disabled".to_owned(),
        Binding::Unknown { raw } => format!(
            "unknown binding {:02X} {:02X} {:02X} {:02X}",
            raw[0], raw[1], raw[2], raw[3]
        ),
    }
}

fn mouse_buttons(mask: u16) -> String {
    let names: Vec<String> = (0..16usize)
        .filter(|&bit| mask & (1 << bit) != 0)
        .map(|bit| {
            MOUSE_BUTTONS.get(bit).map_or_else(
                || format!("mouse button {}", bit + 1),
                |name| (*name).to_owned(),
            )
        })
        .collect();
    if names.is_empty() {
        "no mouse button".to_owned()
    } else {
        names.join(" + ")
    }
}

/// Names for HID keyboard usages (usage page 0x07).
fn key_name(usage: u8) -> String {
    match usage {
        0x04..=0x1D => char::from(b'A' + (usage - 0x04)).to_string(),
        0x1E..=0x26 => char::from(b'1' + (usage - 0x1E)).to_string(),
        0x27 => "0".to_owned(),
        0x28 => "Enter".to_owned(),
        0x29 => "Esc".to_owned(),
        0x2A => "Backspace".to_owned(),
        0x2B => "Tab".to_owned(),
        0x2C => "Space".to_owned(),
        0x3A..=0x45 => format!("F{}", usage - 0x39),
        0x4F => "Right".to_owned(),
        0x50 => "Left".to_owned(),
        0x51 => "Down".to_owned(),
        0x52 => "Up".to_owned(),
        _ => format!("key 0x{usage:02X}"),
    }
}

/// Names for HID consumer-control usages (usage page 0x0C).
fn consumer_name(usage: u16) -> String {
    match usage {
        0x00B5 => "next track".to_owned(),
        0x00B6 => "previous track".to_owned(),
        0x00CD => "play/pause".to_owned(),
        0x00E2 => "mute".to_owned(),
        0x00E9 => "volume up".to_owned(),
        0x00EA => "volume down".to_owned(),
        _ => format!("consumer control 0x{usage:04X}"),
    }
}

fn special_name(action: SpecialAction) -> &'static str {
    match action {
        SpecialAction::TiltLeft => "scroll left",
        SpecialAction::TiltRight => "scroll right",
        SpecialAction::NextDpi => "DPI up",
        SpecialAction::PreviousDpi => "DPI down",
        SpecialAction::CycleDpi => "cycle DPI",
        SpecialAction::DefaultDpi => "default DPI",
        SpecialAction::ShiftDpi => "DPI shift (hold)",
        SpecialAction::NextProfile => "next profile",
        SpecialAction::PreviousProfile => "previous profile",
        SpecialAction::CycleProfile => "cycle profile",
        SpecialAction::GShift => "G-Shift (hold)",
        SpecialAction::BatteryIndicator => "battery indicator",
        SpecialAction::EnableProfile => "switch to profile",
        SpecialAction::PerformanceSwitch => "performance switch",
        SpecialAction::Host => "host",
        SpecialAction::ScrollDown => "scroll down",
        SpecialAction::ScrollUp => "scroll up",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_g502x_gshift_bindings() {
        let label = |raw| binding(&Binding::decode(raw));
        assert_eq!(label([0x80, 0x02, 0x01, 0x17]), "Ctrl+T");
        assert_eq!(label([0x80, 0x02, 0x03, 0x2B]), "Ctrl+Shift+Tab");
        assert_eq!(label([0x80, 0x02, 0x01, 0x27]), "Ctrl+0");
        assert_eq!(label([0x80, 0x03, 0x00, 0xE9]), "volume up");
        assert_eq!(label([0x80, 0x01, 0x00, 0x08]), "back");
        assert_eq!(label([0x90, 0x07, 0x00, 0x00]), "DPI shift (hold)");
        assert_eq!(label([0xFF, 0xFF, 0xFF, 0xFF]), "disabled");
    }

    #[test]
    fn falls_back_to_codes_for_unnamed_values() {
        let label = |raw| binding(&Binding::decode(raw));
        assert_eq!(label([0x80, 0x02, 0x00, 0x68]), "key 0x68");
        assert_eq!(label([0x80, 0x01, 0x00, 0x40]), "mouse button 7");
        assert_eq!(label([0x90, 0x42, 0x00, 0x00]), "firmware action 0x42");
        assert_eq!(
            label([0x80, 0x07, 0x00, 0x00]),
            "unknown binding 80 07 00 00"
        );
    }
}
