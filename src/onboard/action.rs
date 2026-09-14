//! Button actions as typed on the command line.
//!
//! ```text
//! left | right | middle | back | forward | button:N
//! dpi-up | dpi-down | dpi-cycle | dpi-default | dpi-shift | gshift
//! profile-next | profile-previous | profile-cycle
//! scroll-left | scroll-right | scroll-up | scroll-down
//! key:ctrl+shift+t | media:volume-up | disabled
//! ```

use super::format::{Binding, SpecialAction};

const SPECIALS: &[(&str, SpecialAction)] = &[
    ("dpi-up", SpecialAction::NextDpi),
    ("dpi-down", SpecialAction::PreviousDpi),
    ("dpi-cycle", SpecialAction::CycleDpi),
    ("dpi-default", SpecialAction::DefaultDpi),
    ("dpi-shift", SpecialAction::ShiftDpi),
    ("gshift", SpecialAction::GShift),
    ("profile-next", SpecialAction::NextProfile),
    ("profile-previous", SpecialAction::PreviousProfile),
    ("profile-cycle", SpecialAction::CycleProfile),
    ("scroll-left", SpecialAction::TiltLeft),
    ("scroll-right", SpecialAction::TiltRight),
    ("scroll-up", SpecialAction::ScrollUp),
    ("scroll-down", SpecialAction::ScrollDown),
];

const MOUSE_BUTTONS: &[(&str, u16)] = &[
    ("left", 1),
    ("right", 2),
    ("middle", 3),
    ("back", 4),
    ("forward", 5),
];

const MODIFIERS: &[(&str, u8)] = &[("ctrl", 0), ("shift", 1), ("alt", 2), ("super", 3)];

const MEDIA: &[(&str, u16)] = &[
    ("volume-up", 0x00E9),
    ("volume-down", 0x00EA),
    ("mute", 0x00E2),
    ("play-pause", 0x00CD),
    ("next-track", 0x00B5),
    ("previous-track", 0x00B6),
];

impl SpecialAction {
    /// The firmware code [`SpecialAction::from_code`] maps from.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::TiltLeft => 0x01,
            Self::TiltRight => 0x02,
            Self::NextDpi => 0x03,
            Self::PreviousDpi => 0x04,
            Self::CycleDpi => 0x05,
            Self::DefaultDpi => 0x06,
            Self::ShiftDpi => 0x07,
            Self::NextProfile => 0x08,
            Self::PreviousProfile => 0x09,
            Self::CycleProfile => 0x0A,
            Self::GShift => 0x0B,
            Self::BatteryIndicator => 0x0C,
            Self::EnableProfile => 0x0D,
            Self::PerformanceSwitch => 0x0E,
            Self::Host => 0x0F,
            Self::ScrollDown => 0x10,
            Self::ScrollUp => 0x11,
        }
    }
}

/// Parses an action; the error lists what is accepted.
pub fn parse_action(text: &str) -> Result<Binding, String> {
    let text = text.trim().to_ascii_lowercase();
    if text == "disabled" {
        return Ok(Binding::Disabled);
    }
    if let Some(&(_, button)) = MOUSE_BUTTONS.iter().find(|(name, _)| *name == text) {
        return Ok(mouse(button));
    }
    if let Some(&(_, action)) = SPECIALS.iter().find(|(name, _)| *name == text) {
        return Ok(Binding::Special {
            code: action.code(),
            action: Some(action),
            profile: 0,
        });
    }
    if let Some(number) = text.strip_prefix("button:") {
        return match number.parse::<u16>() {
            Ok(button @ 1..=16) => Ok(mouse(button)),
            _ => Err(format!("`{text}`: mouse buttons are button:1 to button:16")),
        };
    }
    if let Some(combo) = text.strip_prefix("key:") {
        return parse_key(combo).map_err(|reason| format!("`{text}`: {reason}"));
    }
    if let Some(name) = text.strip_prefix("media:") {
        return MEDIA
            .iter()
            .find(|(media, _)| *media == name)
            .map(|&(_, usage)| Binding::Consumer { usage })
            .ok_or_else(|| {
                format!(
                    "`{text}`: media actions are {}",
                    MEDIA.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
                )
            });
    }
    Err(format!(
        "unknown action `{text}`; use {}, button:N, {}, key:<combo>, media:<name>, or disabled",
        MOUSE_BUTTONS
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", "),
        SPECIALS
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn mouse(button: u16) -> Binding {
    Binding::Mouse {
        buttons: 1 << (button - 1),
    }
}

fn parse_key(combo: &str) -> Result<Binding, String> {
    let mut parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let key_name = parts
        .pop()
        .filter(|name| !name.is_empty())
        .ok_or("missing key")?;
    let mut modifiers = 0u8;
    for part in parts {
        let &(_, bit) = MODIFIERS
            .iter()
            .find(|(name, _)| *name == part)
            .ok_or_else(|| format!("unknown modifier `{part}`; use ctrl, shift, alt, super"))?;
        modifiers |= 1 << bit;
    }
    let key = key_usage(key_name).ok_or_else(|| {
        format!(
            "unknown key `{key_name}`; use a-z, 0-9, f1-f12, enter, esc, backspace, tab, space, left, right, up, down"
        )
    })?;
    Ok(Binding::Key { modifiers, key })
}

/// HID keyboard usage for a key name.
fn key_usage(name: &str) -> Option<u8> {
    let bytes = name.as_bytes();
    Some(match (name, bytes) {
        (_, [letter @ b'a'..=b'z']) => 0x04 + (letter - b'a'),
        (_, [b'0']) => 0x27,
        (_, [digit @ b'1'..=b'9']) => 0x1E + (digit - b'1'),
        ("enter", _) => 0x28,
        ("esc", _) => 0x29,
        ("backspace", _) => 0x2A,
        ("tab", _) => 0x2B,
        ("space", _) => 0x2C,
        ("right", _) => 0x4F,
        ("left", _) => 0x50,
        ("down", _) => 0x51,
        ("up", _) => 0x52,
        _ => {
            let number: u8 = name.strip_prefix('f')?.parse().ok()?;
            if !(1..=12).contains(&number) {
                return None;
            }
            0x39 + number
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboard::label;

    fn label_of(text: &str) -> String {
        label::binding(&parse_action(text).expect("parses"))
    }

    #[test]
    fn parses_mouse_buttons() {
        assert_eq!(label_of("back"), "back");
        assert_eq!(label_of("Forward"), "forward");
        assert_eq!(label_of("button:7"), "mouse button 7");
        assert!(parse_action("button:0").is_err());
        assert!(parse_action("button:17").is_err());
    }

    #[test]
    fn parses_firmware_actions() {
        assert_eq!(label_of("dpi-shift"), "DPI shift (hold)");
        assert_eq!(label_of("gshift"), "G-Shift (hold)");
        assert_eq!(label_of("profile-cycle"), "cycle profile");
        assert_eq!(label_of("scroll-left"), "scroll left");
    }

    #[test]
    fn parses_key_combos_like_the_device_stores_them() {
        assert_eq!(
            parse_action("key:ctrl+t"),
            Ok(Binding::Key {
                modifiers: 0x01,
                key: 0x17
            })
        );
        assert_eq!(label_of("key:ctrl+shift+tab"), "Ctrl+Shift+Tab");
        assert_eq!(label_of("key:super+f12"), "Super+F12");
        assert_eq!(label_of("key:0"), "0");
    }

    #[test]
    fn parses_media_keys() {
        assert_eq!(label_of("media:volume-up"), "volume up");
        assert_eq!(label_of("media:play-pause"), "play/pause");
    }

    #[test]
    fn explains_bad_input() {
        let unknown = parse_action("jump").expect_err("unknown");
        assert!(
            unknown.contains("dpi-up") && unknown.contains("key:<combo>"),
            "{unknown}"
        );
        assert!(
            parse_action("key:hyper+t")
                .expect_err("modifier")
                .contains("modifier")
        );
        assert!(
            parse_action("key:ctrl+")
                .expect_err("empty")
                .contains("missing key")
        );
        assert!(
            parse_action("key:f13")
                .expect_err("f13")
                .contains("unknown key")
        );
        assert!(
            parse_action("media:louder")
                .expect_err("media")
                .contains("volume-up")
        );
    }

    #[test]
    fn special_codes_round_trip() {
        for code in 0x01..=0x11 {
            let action = SpecialAction::from_code(code).expect("known code");
            assert_eq!(action.code(), code);
        }
    }
}
