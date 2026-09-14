//! Decoding of HID++ 2.0 `OnboardProfiles` (0x8100) memory.
//!
//! Byte layouts follow libratbag (`src/hidpp20.c`, `union hidpp20_internal_profile`;
//! `src/hidpp20.h`, button bindings). Profiles are only decoded for layouts in
//! [`VERIFIED_LAYOUTS`], which were checked against real hardware.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Sector holding the directory of user profiles.
pub const USER_DIRECTORY_SECTOR: u16 = 0x0000;
/// Sector holding the directory of factory (ROM) profiles.
pub const ROM_DIRECTORY_SECTOR: u16 = 0x0100;
/// Bytes returned by one `memoryRead` call.
pub const READ_CHUNK: usize = 16;

/// `(memory model, profile format)` pairs whose profile layout was verified on a device.
///
/// `(1, 4)`: wired G502 X (046d:c099), firmware U1 60.00.B0009.
pub const VERIFIED_LAYOUTS: &[(u8, u8)] = &[(1, 4)];

/// `(memory model, profile format)` pairs Omalogi reads and edits. libratbag lays out all
/// five formats with one struct (`union hidpp20_internal_profile`): report rate, DPI
/// stages, both button tables and the name share their offsets, and only LED fields
/// differ, which Omalogi never changes. Layouts outside [`VERIFIED_LAYOUTS`] are untested.
pub const DECODABLE_LAYOUTS: &[(u8, u8)] = &[(1, 1), (1, 2), (1, 3), (1, 4), (1, 5)];

const DESCRIPTION_LEN: usize = 11;
const DIRECTORY_END: u16 = 0xFFFF;
pub(crate) const DIRECTORY_ENTRY_LEN: usize = 4;
pub(crate) const DPI_STAGE_COUNT: usize = 5;
pub(crate) const DPI_OFFSET: usize = 3;
pub(crate) const BUTTON_SLOTS: usize = 16;
pub(crate) const BINDING_LEN: usize = 4;
pub(crate) const BUTTON_OFFSET: usize = 32;
pub(crate) const GSHIFT_BUTTON_OFFSET: usize = BUTTON_OFFSET + BUTTON_SLOTS * BINDING_LEN;
pub(crate) const NAME_OFFSET: usize = GSHIFT_BUTTON_OFFSET + BUTTON_SLOTS * BINDING_LEN;
pub(crate) const NAME_LEN: usize = 48;
pub(crate) const MIN_SECTOR_LEN: usize = NAME_OFFSET + NAME_LEN;
const MAX_SECTOR_LEN: usize = 4096;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("expected at least {expected} bytes, got {actual}")]
    TooShort { expected: usize, actual: usize },
    #[error("sector size {0} is outside the supported range")]
    UnsupportedSectorSize(u16),
    #[error(
        "profile layout (memory model {memory_model}, format {profile_format}) \
         is not one Omalogi can read"
    )]
    UnsupportedLayout {
        memory_model: u8,
        profile_format: u8,
    },
}

fn ensure_len(data: &[u8], expected: usize) -> Result<(), DecodeError> {
    if data.len() < expected {
        return Err(DecodeError::TooShort {
            expected,
            actual: data.len(),
        });
    }
    Ok(())
}

/// The reply to `getDescription`: how the device lays out its profile memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Description {
    pub memory_model: u8,
    pub profile_format: u8,
    pub macro_format: u8,
    pub profile_count: u8,
    pub rom_profile_count: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
    pub mechanical_layout: u8,
    pub various_info: u8,
}

impl Description {
    pub fn parse(payload: &[u8]) -> Result<Self, DecodeError> {
        ensure_len(payload, DESCRIPTION_LEN)?;
        let sector_size = u16::from_be_bytes([payload[7], payload[8]]);
        if !(MIN_SECTOR_LEN..=MAX_SECTOR_LEN).contains(&usize::from(sector_size)) {
            return Err(DecodeError::UnsupportedSectorSize(sector_size));
        }
        Ok(Self {
            memory_model: payload[0],
            profile_format: payload[1],
            macro_format: payload[2],
            profile_count: payload[3],
            rom_profile_count: payload[4],
            button_count: payload[5],
            sector_count: payload[6],
            sector_size,
            mechanical_layout: payload[9],
            various_info: payload[10],
        })
    }

    /// Whether this device's profile layout has been verified on hardware.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        VERIFIED_LAYOUTS.contains(&(self.memory_model, self.profile_format))
    }

    /// Whether Omalogi can read and edit this device's profile layout.
    #[must_use]
    pub fn is_decodable(&self) -> bool {
        DECODABLE_LAYOUTS.contains(&(self.memory_model, self.profile_format))
    }
}

/// CRC-16-CCITT (initial value 0xFFFF, polynomial 0x1021), as used by profile sectors.
#[must_use]
pub fn crc_ccitt(data: &[u8]) -> u16 {
    data.iter().fold(0xFFFF, |crc, &byte| {
        (0..8).fold(crc ^ (u16::from(byte) << 8), |crc, _| {
            if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            }
        })
    })
}

/// Whether a sector ends in a valid big-endian CRC over the bytes before it.
///
/// Factory (ROM) sectors on the G502 X store `0000`/`FFFF` instead of a CRC.
#[must_use]
pub fn sector_crc_valid(sector: &[u8]) -> bool {
    let Some((body, crc)) = sector.split_last_chunk::<2>() else {
        return false;
    };
    crc_ccitt(body) == u16::from_be_bytes(*crc)
}

/// One entry of a profile directory sector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DirectoryEntry {
    pub sector: u16,
    pub enabled: bool,
}

/// Parses a directory sector: `[sector u16 BE, enabled, reserved]` entries up to `0xFFFF`.
#[must_use]
pub fn parse_directory(sector: &[u8], max_entries: usize) -> Vec<DirectoryEntry> {
    let body = sector
        .split_last_chunk::<2>()
        .map_or(sector, |(body, _)| body);
    body.as_chunks::<DIRECTORY_ENTRY_LEN>()
        .0
        .iter()
        .map(|&[hi, lo, enabled, _]| (u16::from_be_bytes([hi, lo]), enabled))
        .take_while(|&(sector, _)| sector != DIRECTORY_END)
        .take(max_entries)
        .map(|(sector, enabled)| DirectoryEntry {
            sector,
            enabled: enabled != 0,
        })
        .collect()
}

/// Maps `getCurrentProfile`'s index to a position in the profile directory.
///
/// The index is 1-based on the wired G502 X (verified on hardware; libratbag's
/// `INDEX_OFFSET` quirk), so 0 has no directory entry.
#[must_use]
pub fn current_profile_position(raw_index: u8) -> Option<usize> {
    raw_index.checked_sub(1).map(usize::from)
}

/// Firmware actions a button can be bound to (libratbag `HIDPP20_BUTTON_SPECIAL_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecialAction {
    TiltLeft,
    TiltRight,
    NextDpi,
    PreviousDpi,
    CycleDpi,
    DefaultDpi,
    ShiftDpi,
    NextProfile,
    PreviousProfile,
    CycleProfile,
    GShift,
    BatteryIndicator,
    EnableProfile,
    PerformanceSwitch,
    Host,
    ScrollDown,
    ScrollUp,
}

impl SpecialAction {
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0x01 => Self::TiltLeft,
            0x02 => Self::TiltRight,
            0x03 => Self::NextDpi,
            0x04 => Self::PreviousDpi,
            0x05 => Self::CycleDpi,
            0x06 => Self::DefaultDpi,
            0x07 => Self::ShiftDpi,
            0x08 => Self::NextProfile,
            0x09 => Self::PreviousProfile,
            0x0A => Self::CycleProfile,
            0x0B => Self::GShift,
            0x0C => Self::BatteryIndicator,
            0x0D => Self::EnableProfile,
            0x0E => Self::PerformanceSwitch,
            0x0F => Self::Host,
            0x10 => Self::ScrollDown,
            0x11 => Self::ScrollUp,
            _ => return None,
        })
    }
}

/// What one button slot in a profile does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Binding {
    /// Mouse buttons as a HID button bitmask (bit 0 = button 1).
    Mouse {
        buttons: u16,
    },
    /// A key plus modifier flags laid out as in a HID keyboard report (bit 0 = left Ctrl).
    Key {
        modifiers: u8,
        key: u8,
    },
    /// A HID consumer-control usage, e.g. `0x00E9` volume up.
    Consumer {
        usage: u16,
    },
    /// A firmware action; `action` is `None` for codes not known to libratbag.
    /// `profile` is the target of profile actions such as `EnableProfile`.
    Special {
        code: u8,
        action: Option<SpecialAction>,
        profile: u8,
    },
    /// A reference into macro memory; macro contents are not decoded yet.
    Macro {
        raw: [u8; BINDING_LEN],
    },
    Disabled,
    Unknown {
        raw: [u8; BINDING_LEN],
    },
}

impl Binding {
    #[must_use]
    pub fn decode(raw: [u8; BINDING_LEN]) -> Self {
        match raw {
            [0x80, 0x01, hi, lo] => Self::Mouse {
                buttons: u16::from_be_bytes([hi, lo]),
            },
            [0x80, 0x02, modifiers, key] => Self::Key { modifiers, key },
            [0x80, 0x03, hi, lo] => Self::Consumer {
                usage: u16::from_be_bytes([hi, lo]),
            },
            [0x90, code, _, profile] => Self::Special {
                code,
                action: SpecialAction::from_code(code),
                profile,
            },
            [0x00, ..] => Self::Macro { raw },
            [0xFF, ..] => Self::Disabled,
            _ => Self::Unknown { raw },
        }
    }
}

/// One onboard profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Profile {
    /// Report interval in milliseconds (1 = 1000 Hz).
    pub report_rate_ms: u8,
    pub default_dpi_index: u8,
    pub shift_dpi_index: u8,
    /// DPI stages; `None` marks an unused stage (stored as 0 or 0xFFFF).
    pub dpi_stages: [Option<u16>; DPI_STAGE_COUNT],
    pub buttons: [Binding; BUTTON_SLOTS],
    /// Bindings while G-Shift is held.
    pub gshift_buttons: [Binding; BUTTON_SLOTS],
    pub name: Option<String>,
}

impl Profile {
    pub fn parse(sector: &[u8], description: &Description) -> Result<Self, DecodeError> {
        if !description.is_decodable() {
            return Err(DecodeError::UnsupportedLayout {
                memory_model: description.memory_model,
                profile_format: description.profile_format,
            });
        }
        ensure_len(sector, MIN_SECTOR_LEN)?;
        let dpi_stages = std::array::from_fn(|stage| {
            let at = DPI_OFFSET + stage * 2;
            match u16::from_le_bytes([sector[at], sector[at + 1]]) {
                0 | 0xFFFF => None,
                dpi => Some(dpi),
            }
        });
        Ok(Self {
            report_rate_ms: sector[0],
            default_dpi_index: sector[1],
            shift_dpi_index: sector[2],
            dpi_stages,
            buttons: bindings_at(sector, BUTTON_OFFSET),
            gshift_buttons: bindings_at(sector, GSHIFT_BUTTON_OFFSET),
            name: decode_name(&sector[NAME_OFFSET..NAME_OFFSET + NAME_LEN]),
        })
    }
}

fn bindings_at(sector: &[u8], offset: usize) -> [Binding; BUTTON_SLOTS] {
    std::array::from_fn(|slot| {
        let at = offset + slot * BINDING_LEN;
        Binding::decode([sector[at], sector[at + 1], sector[at + 2], sector[at + 3]])
    })
}

pub(crate) fn decode_name(raw: &[u8]) -> Option<String> {
    let end = raw
        .iter()
        .position(|&byte| byte == 0x00 || byte == 0xFF)
        .unwrap_or(raw.len());
    let name = String::from_utf8_lossy(&raw[..end]).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/g502x-c099.json");

    fn fixture_hex(pointer: &str) -> Vec<u8> {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture is JSON");
        let hex = fixture
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("fixture has no string at {pointer}"));
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("fixture hex"))
            .collect()
    }

    fn sector(id: &str) -> Vec<u8> {
        fixture_hex(&format!("/onboard/sectors/{id}"))
    }

    fn description() -> Description {
        Description::parse(&fixture_hex("/onboard/description")).expect("description parses")
    }

    fn special(code: u8) -> Binding {
        Binding::Special {
            code,
            action: SpecialAction::from_code(code),
            profile: 0,
        }
    }

    fn mouse(buttons: u16) -> Binding {
        Binding::Mouse { buttons }
    }

    fn key(modifiers: u8, key: u8) -> Binding {
        Binding::Key { modifiers, key }
    }

    #[test]
    fn crc_matches_reference_check_value() {
        assert_eq!(crc_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn parses_g502x_description() {
        let d = description();
        assert_eq!(
            (d.memory_model, d.profile_format, d.macro_format),
            (1, 4, 1)
        );
        assert_eq!(
            (d.profile_count, d.rom_profile_count, d.button_count),
            (5, 2, 11)
        );
        assert_eq!((d.sector_count, d.sector_size), (16, 255));
        assert!(d.is_verified());
    }

    #[test]
    fn rejects_short_description() {
        assert_eq!(
            Description::parse(&[1, 4, 1]),
            Err(DecodeError::TooShort {
                expected: 11,
                actual: 3
            })
        );
    }

    #[test]
    fn user_directory_lists_two_enabled_profiles() {
        let directory = sector("0000");
        assert!(sector_crc_valid(&directory));
        let entries: Vec<_> = parse_directory(&directory, 5)
            .iter()
            .map(|e| (e.sector, e.enabled))
            .collect();
        assert_eq!(
            entries,
            [(1, true), (2, true), (3, false), (4, false), (5, false)]
        );
    }

    #[test]
    fn user_sectors_have_valid_crcs_and_rom_sectors_do_not() {
        for id in ["0001", "0002", "0003", "0004", "0005"] {
            assert!(sector_crc_valid(&sector(id)), "sector {id}");
        }
        for id in ["0100", "0101", "0102"] {
            assert!(!sector_crc_valid(&sector(id)), "sector {id}");
        }
    }

    #[test]
    fn decodes_dpi_shift_profile() {
        let p = Profile::parse(&sector("0001"), &description()).expect("profile parses");
        assert_eq!(p.report_rate_ms, 1);
        assert_eq!((p.default_dpi_index, p.shift_dpi_index), (2, 0));
        assert_eq!(
            p.dpi_stages,
            [Some(800), Some(1200), Some(1600), Some(2400), Some(3200)]
        );
        assert_eq!(
            p.buttons[..11],
            [
                mouse(0x01),
                mouse(0x02),
                mouse(0x04),
                mouse(0x08),
                special(0x07),
                mouse(0x10),
                special(0x01),
                special(0x02),
                special(0x0A),
                special(0x03),
                special(0x04),
            ]
        );
        assert!(p.buttons[11..].iter().all(|b| *b == Binding::Disabled));
        assert!(p.gshift_buttons.iter().all(|b| *b == Binding::Disabled));
        assert_eq!(p.name, None);
    }

    #[test]
    fn decodes_gshift_profile() {
        let p = Profile::parse(&sector("0002"), &description()).expect("profile parses");
        assert_eq!(p.buttons[4], special(0x0B));
        assert_eq!(
            (p.buttons[11], p.buttons[12]),
            (special(0x10), special(0x11))
        );
        assert_eq!(
            p.gshift_buttons,
            [
                mouse(0x01),
                mouse(0x02),
                key(0x01, 0x17),
                mouse(0x08),
                Binding::Disabled,
                mouse(0x10),
                key(0x03, 0x2B),
                key(0x01, 0x2B),
                key(0x01, 0x27),
                key(0x01, 0x1D),
                key(0x01, 0x1B),
                Binding::Consumer { usage: 0xEA },
                Binding::Consumer { usage: 0xE9 },
                Binding::Disabled,
                Binding::Disabled,
                Binding::Disabled,
            ]
        );
    }

    #[test]
    fn reads_every_libratbag_layout_but_refuses_others() {
        let mut d = description();
        d.profile_format = 5;
        assert!(!d.is_verified());
        assert!(d.is_decodable());
        assert!(Profile::parse(&sector("0001"), &d).is_ok());
        d.profile_format = 6;
        assert_eq!(
            Profile::parse(&sector("0001"), &d),
            Err(DecodeError::UnsupportedLayout {
                memory_model: 1,
                profile_format: 6
            })
        );
    }

    #[test]
    fn current_profile_index_is_one_based() {
        let raw = fixture_hex("/onboard/current_profile");
        assert_eq!(current_profile_position(raw[1]), Some(0));
        assert_eq!(current_profile_position(0), None);
    }
}
