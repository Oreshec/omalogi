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

/// One entry of the action list offered to users, e.g. in the shell plugin.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActionInfo {
    /// The text [`parse_action`] accepts. `key:` alone asks for a shortcut to type.
    pub value: String,
    pub label: String,
    pub group: &'static str,
}

/// Every action a button can be bound to, grouped for display.
#[must_use]
pub fn catalog() -> Vec<ActionInfo> {
    let entry = |value: String, group: &'static str| {
        let label = parse_action(&value)
            .map(|binding| super::label::binding(&binding))
            .expect("catalog values parse");
        ActionInfo {
            value,
            label,
            group,
        }
    };
    let mut actions: Vec<ActionInfo> = MOUSE_BUTTONS
        .iter()
        .map(|(name, _)| entry((*name).to_owned(), "Mouse"))
        .collect();
    for (name, action) in SPECIALS {
        let group = match action {
            SpecialAction::TiltLeft
            | SpecialAction::TiltRight
            | SpecialAction::ScrollUp
            | SpecialAction::ScrollDown => "Scroll",
            SpecialAction::NextProfile
            | SpecialAction::PreviousProfile
            | SpecialAction::CycleProfile => "Profiles",
            SpecialAction::GShift => "Other",
            _ => "DPI",
        };
        actions.push(entry((*name).to_owned(), group));
    }
    actions.extend(
        MEDIA
            .iter()
            .map(|(name, _)| entry(format!("media:{name}"), "Media")),
    );
    actions.push(ActionInfo {
        value: "key:".to_owned(),
        label: "Keyboard shortcut…".to_owned(),
        group: "Keyboard",
    });
    actions.push(entry("disabled".to_owned(), "Other"));
    actions
}

/// The action text [`parse_action`] reads back into `binding`, or `None` for bindings
/// that cannot be typed: macros, unknown encodings, right-hand modifiers, and several
/// mouse buttons at once.
#[must_use]
pub fn action_text(binding: &Binding) -> Option<String> {
    match *binding {
        Binding::Disabled => Some("disabled".to_owned()),
        Binding::Mouse { buttons } if buttons.count_ones() == 1 => {
            let button = u16::try_from(buttons.trailing_zeros()).expect("below 16") + 1;
            Some(
                MOUSE_BUTTONS
                    .iter()
                    .find(|(_, number)| *number == button)
                    .map_or_else(
                        || format!("button:{button}"),
                        |(name, _)| (*name).to_owned(),
                    ),
            )
        }
        Binding::Special {
            action: Some(action),
            profile: 0,
            ..
        } => SPECIALS
            .iter()
            .find(|(_, special)| *special == action)
            .map(|(name, _)| (*name).to_owned()),
        Binding::Consumer { usage } => MEDIA
            .iter()
            .find(|(_, media)| *media == usage)
            .map(|(name, _)| format!("media:{name}")),
        Binding::Key { modifiers, key } => {
            if modifiers & 0xF0 != 0 {
                return None;
            }
            let mut parts: Vec<String> = MODIFIERS
                .iter()
                .filter(|(_, bit)| modifiers & (1 << bit) != 0)
                .map(|(name, _)| (*name).to_owned())
                .collect();
            parts.push(key_text(key)?);
            Some(format!("key:{}", parts.join("+")))
        }
        _ => None,
    }
}

/// The key name [`key_usage`] maps from.
fn key_text(usage: u8) -> Option<String> {
    Some(match usage {
        0x04..=0x1D => char::from(b'a' + (usage - 0x04)).to_string(),
        0x1E..=0x26 => char::from(b'1' + (usage - 0x1E)).to_string(),
        0x27 => "0".to_owned(),
        0x28 => "enter".to_owned(),
        0x29 => "esc".to_owned(),
        0x2A => "backspace".to_owned(),
        0x2B => "tab".to_owned(),
        0x2C => "space".to_owned(),
        0x3A..=0x45 => format!("f{}", usage - 0x39),
        0x4F => "right".to_owned(),
        0x50 => "left".to_owned(),
        0x51 => "down".to_owned(),
        0x52 => "up".to_owned(),
        _ => return None,
    })
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
    fn catalog_values_parse_to_their_labels() {
        let actions = catalog();
        let mut values: Vec<&str> = actions.iter().map(|a| a.value.as_str()).collect();
        values.sort_unstable();
        values.dedup();
        assert_eq!(values.len(), actions.len(), "values are unique");
        for action in actions.iter().filter(|a| a.value != "key:") {
            assert_eq!(label_of(&action.value), action.label, "{}", action.value);
        }
        assert!(
            actions
                .iter()
                .any(|a| a.value == "key:" && a.group == "Keyboard")
        );
        assert!(
            actions
                .iter()
                .any(|a| a.value == "dpi-shift" && a.group == "DPI")
        );
        assert!(
            actions
                .iter()
                .any(|a| a.value == "media:mute" && a.group == "Media")
        );
    }

    #[test]
    fn action_text_reads_back_into_the_same_binding() {
        for action in catalog().iter().filter(|a| a.value != "key:") {
            let binding = parse_action(&action.value).expect("catalog value parses");
            assert_eq!(
                action_text(&binding).as_deref(),
                Some(action.value.as_str())
            );
        }
        for text in [
            "key:ctrl+t",
            "key:ctrl+shift+tab",
            "key:super+f12",
            "key:alt+0",
            "button:7",
        ] {
            let binding = parse_action(text).expect("parses");
            assert_eq!(action_text(&binding).as_deref(), Some(text));
        }
    }

    #[test]
    fn action_text_names_the_g502x_gshift_bindings() {
        let text = |raw| action_text(&Binding::decode(raw));
        assert_eq!(
            text([0x80, 0x02, 0x01, 0x17]).as_deref(),
            Some("key:ctrl+t")
        );
        assert_eq!(
            text([0x80, 0x02, 0x03, 0x2B]).as_deref(),
            Some("key:ctrl+shift+tab")
        );
        assert_eq!(
            text([0x80, 0x03, 0x00, 0xEA]).as_deref(),
            Some("media:volume-down")
        );
        assert_eq!(text([0x90, 0x0B, 0x00, 0x00]).as_deref(), Some("gshift"));
    }

    #[test]
    fn untypeable_bindings_have_no_action_text() {
        let text = |raw| action_text(&Binding::decode(raw));
        assert_eq!(text([0x00, 0x01, 0x00, 0x10]), None, "macro");
        assert_eq!(text([0x80, 0x07, 0x00, 0x00]), None, "unknown encoding");
        assert_eq!(text([0x80, 0x02, 0x10, 0x17]), None, "right ctrl");
        assert_eq!(text([0x80, 0x01, 0x00, 0x03]), None, "two mouse buttons");
        assert_eq!(text([0x90, 0x0C, 0x00, 0x00]), None, "battery indicator");
        assert_eq!(text([0x80, 0x02, 0x00, 0x68]), None, "unnamed key");
    }

    #[test]
    fn special_codes_round_trip() {
        for code in 0x01..=0x11 {
            let action = SpecialAction::from_code(code).expect("known code");
            assert_eq!(action.code(), code);
        }
    }
}
