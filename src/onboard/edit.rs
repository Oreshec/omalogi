//! Editing onboard profile sectors in place.
//!
//! An edit patches only the bytes whose decoded value changes, in a sector read from
//! the device, then recomputes the CRC. Bytes this crate does not decode survive
//! unchanged, unlike rebuilding the sector from a struct.

use std::ops::Range;

use super::format::{
    BINDING_LEN, BUTTON_OFFSET, BUTTON_SLOTS, Binding, DIRECTORY_ENTRY_LEN, DPI_OFFSET,
    DPI_STAGE_COUNT, DecodeError, Description, GSHIFT_BUTTON_OFFSET, MIN_SECTOR_LEN, NAME_LEN,
    NAME_OFFSET, crc_ccitt, decode_name,
};

/// The longest profile name: the 48-byte field keeps a terminating NUL, as libratbag writes it.
pub const MAX_NAME_LEN: usize = NAME_LEN - 1;

/// A directory sector with profile `position`'s enabled flag set and a new CRC. Entries are
/// `[0x00, profile number, enabled, 0x00]`; only the flag byte and the CRC change.
///
/// # Panics
///
/// When `position` is outside the directory; callers validate it first.
#[must_use]
pub fn directory_with_enabled(directory: &[u8], position: usize, enabled: bool) -> Vec<u8> {
    let mut data = directory.to_vec();
    data[position * DIRECTORY_ENTRY_LEN + 2] = u8::from(enabled);
    let crc_at = data.len() - 2;
    let crc = crc_ccitt(&data[..crc_at]);
    data[crc_at..].copy_from_slice(&crc.to_be_bytes());
    data
}

impl Binding {
    /// The 4-byte encoding that [`Binding::decode`] reads.
    #[must_use]
    pub fn encode(&self) -> [u8; BINDING_LEN] {
        match *self {
            Self::Mouse { buttons } => {
                let [hi, lo] = buttons.to_be_bytes();
                [0x80, 0x01, hi, lo]
            }
            Self::Key { modifiers, key } => [0x80, 0x02, modifiers, key],
            Self::Consumer { usage } => {
                let [hi, lo] = usage.to_be_bytes();
                [0x80, 0x03, hi, lo]
            }
            Self::Special {
                code,
                reserved,
                profile,
                ..
            } => [0x90, code, reserved, profile],
            Self::Macro { raw } | Self::Unknown { raw } => raw,
            Self::Disabled => [0xFF; BINDING_LEN],
        }
    }

    /// Whether `other` is this binding spelled the way the user would name it: the same
    /// action, ignoring the bytes of a firmware action that no action text carries.
    ///
    /// It is what tells a slot's action apart from the bytes it stores it with, since a
    /// G502 Hero holds `90 xx ff ff` and a G502 X Lightspeed `90 xx ff 00` where a wired
    /// G502 X holds `90 xx 00 00`, and all three are the same action. Those bytes are
    /// padding only for actions that are known not to use them: `EnableProfile` keeps the
    /// profile it switches to in the last byte, and an unknown code may use either.
    #[must_use]
    pub fn same_action(&self, other: &Self) -> bool {
        match (*self, *other) {
            (
                Self::Special {
                    code,
                    action,
                    reserved,
                    profile,
                },
                Self::Special {
                    code: other_code,
                    reserved: other_reserved,
                    profile: other_profile,
                    ..
                },
            ) => {
                code == other_code
                    && match action {
                        None => reserved == other_reserved && profile == other_profile,
                        Some(action) if action.targets_profile() => profile == other_profile,
                        Some(_) => true,
                    }
            }
            _ => self == other,
        }
    }
}

/// The two binding tables of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Buttons,
    GShift,
}

/// Where a slot's four bytes are stored in a profile sector, wherever the layout puts
/// them. Reads a slot with it: `&sector[binding_range(table, slot)]`, the way
/// [`Binding::decode`] takes it. Slicing the sector is what reports a slot past its end.
#[must_use]
pub fn binding_range(table: Table, slot: usize) -> Range<usize> {
    let base = match table {
        Table::Buttons => BUTTON_OFFSET,
        Table::GShift => GSHIFT_BUTTON_OFFSET,
    };
    let at = base + slot * BINDING_LEN;
    at..at + BINDING_LEN
}

/// A profile sector being edited.
#[derive(Debug, Clone)]
pub struct ProfileEditor {
    data: Vec<u8>,
}

impl ProfileEditor {
    /// Starts from a sector as read from the device.
    pub fn new(sector: &[u8], description: &Description) -> Result<Self, DecodeError> {
        if !description.is_decodable() {
            return Err(DecodeError::UnsupportedLayout {
                memory_model: description.memory_model,
                profile_format: description.profile_format,
            });
        }
        if sector.len() < MIN_SECTOR_LEN {
            return Err(DecodeError::TooShort {
                expected: MIN_SECTOR_LEN,
                actual: sector.len(),
            });
        }
        Ok(Self {
            data: sector.to_vec(),
        })
    }

    /// Report interval in milliseconds (1 = 1000 Hz).
    pub fn set_report_rate_ms(&mut self, interval_ms: u8) {
        self.data[0] = interval_ms;
    }

    pub fn set_default_dpi_index(&mut self, index: u8) {
        self.data[1] = index;
    }

    pub fn set_shift_dpi_index(&mut self, index: u8) {
        self.data[2] = index;
    }

    /// Sets all DPI stages; `None` stages are stored as 0.
    pub fn set_dpi_stages(&mut self, stages: [Option<u16>; DPI_STAGE_COUNT]) {
        for (stage, dpi) in stages.into_iter().enumerate() {
            let at = DPI_OFFSET + stage * 2;
            let current = match u16::from_le_bytes([self.data[at], self.data[at + 1]]) {
                0 | 0xFFFF => None,
                value => Some(value),
            };
            if current != dpi {
                self.data[at..at + 2].copy_from_slice(&dpi.unwrap_or(0).to_le_bytes());
            }
        }
    }

    /// Binds `slot` (below 16) in `table`.
    ///
    /// A slot is written only when the action changes. The bytes a binding decodes from
    /// and returns are kept as read, so a firmware action written with its own text, whose
    /// `reserved` and profile bytes text cannot spell, leaves the ones the mouse stores
    /// (`ffff` on a G502 Hero, `ff00` on a G502 X Lightspeed) alone.
    ///
    /// # Panics
    ///
    /// When `slot` is 16 or higher; callers validate slots first.
    pub fn set_binding(&mut self, table: Table, slot: usize, binding: Binding) {
        assert!(slot < BUTTON_SLOTS, "binding slot {slot} out of range");
        let at = binding_range(table, slot);
        let raw: [u8; BINDING_LEN] = self.data[at.clone()]
            .try_into()
            .expect("slice has binding length");
        if Binding::decode(raw).same_action(&binding) {
            return;
        }
        self.data[at].copy_from_slice(&binding.encode());
    }

    /// Sets the profile name, printable ASCII of at most [`MAX_NAME_LEN`] bytes that the
    /// caller has validated, stored NUL-padded as libratbag writes it. `None` clears it to
    /// the unwritten state, all 0xFF. The field is left alone when the name is unchanged.
    ///
    /// # Panics
    ///
    /// When `name` is longer than [`MAX_NAME_LEN`] bytes.
    pub fn set_name(&mut self, name: Option<&str>) {
        let name = name.filter(|name| !name.is_empty());
        let field = NAME_OFFSET..NAME_OFFSET + NAME_LEN;
        if decode_name(&self.data[field.clone()]).as_deref() == name {
            return;
        }
        match name {
            Some(name) => {
                assert!(name.len() <= MAX_NAME_LEN, "profile name too long");
                self.data[field.clone()].fill(0);
                self.data[NAME_OFFSET..NAME_OFFSET + name.len()].copy_from_slice(name.as_bytes());
            }
            None => self.data[field].fill(0xFF),
        }
    }

    /// The edited sector, ending in a freshly computed CRC.
    #[must_use]
    pub fn finish(mut self) -> Vec<u8> {
        let crc_at = self.data.len() - 2;
        let crc = crc_ccitt(&self.data[..crc_at]);
        self.data[crc_at..].copy_from_slice(&crc.to_be_bytes());
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboard::{
        action::parse_action,
        format::{Profile, SpecialAction, crc_ccitt, sector_crc_valid},
    };

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

    fn description() -> Description {
        Description::parse(&fixture_hex("/onboard/description")).expect("description")
    }

    fn changed_offsets(a: &[u8], b: &[u8]) -> Vec<usize> {
        a.iter()
            .zip(b)
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .map(|(i, _)| i)
            .collect()
    }

    /// Where a sector's two checksum bytes are.
    fn checksum(sector_len: usize) -> Range<usize> {
        sector_len - 2..sector_len
    }

    /// A sector of profile 4, with `raw` in `slot` and a checksum that matches.
    fn sector_with_binding(slot: usize, raw: [u8; BINDING_LEN]) -> Vec<u8> {
        let mut sector = fixture_hex("/onboard/sectors/0004");
        let at = BUTTON_OFFSET + slot * BINDING_LEN;
        sector[at..at + BINDING_LEN].copy_from_slice(&raw);
        let crc_at = sector.len() - 2;
        let crc = crc_ccitt(&sector[..crc_at]);
        sector[crc_at..].copy_from_slice(&crc.to_be_bytes());
        sector
    }

    #[test]
    fn encoding_round_trips_every_real_binding() {
        for id in ["0001", "0002", "0003", "0004", "0005", "0101", "0102"] {
            let sector = fixture_hex(&format!("/onboard/sectors/{id}"));
            for table in [BUTTON_OFFSET, GSHIFT_BUTTON_OFFSET] {
                for slot in 0..BUTTON_SLOTS {
                    let at = table + slot * BINDING_LEN;
                    let raw: [u8; BINDING_LEN] =
                        sector[at..at + BINDING_LEN].try_into().expect("4");
                    assert_eq!(
                        Binding::decode(raw).encode(),
                        raw,
                        "sector {id} offset {at}"
                    );
                }
            }
        }
    }

    #[test]
    fn special_bindings_keep_their_reserved_byte() {
        for raw in [
            [0x90, 0x07, 0xFF, 0x00],
            [0x90, 0x03, 0xFF, 0xFF],
            [0x90, 0x0B, 0x00, 0x00],
        ] {
            assert_eq!(Binding::decode(raw).encode(), raw);
        }
    }

    #[test]
    fn padding_is_ignored_only_where_an_action_does_not_use_it() {
        let same = |a: [u8; BINDING_LEN], b: [u8; BINDING_LEN]| {
            Binding::decode(a).same_action(&Binding::decode(b))
        };
        // DPI shift: the reserved and profile bytes are padding.
        assert!(same([0x90, 0x07, 0xFF, 0xFF], [0x90, 0x07, 0x00, 0x00]));
        assert!(same([0x90, 0x07, 0xFF, 0x00], [0x90, 0x07, 0x00, 0x00]));
        assert!(!same([0x90, 0x07, 0x00, 0x00], [0x90, 0x03, 0x00, 0x00]));
        // EnableProfile: the last byte is the profile it switches to.
        assert!(same([0x90, 0x0D, 0xFF, 0x02], [0x90, 0x0D, 0x00, 0x02]));
        assert!(!same([0x90, 0x0D, 0x00, 0x01], [0x90, 0x0D, 0x00, 0x02]));
        // An unknown code: no byte is known to be padding.
        assert!(same([0x90, 0x42, 0x01, 0x02], [0x90, 0x42, 0x01, 0x02]));
        assert!(!same([0x90, 0x42, 0x01, 0x02], [0x90, 0x42, 0x00, 0x02]));
        assert!(!same([0x90, 0x42, 0x01, 0x02], [0x90, 0x42, 0x01, 0x03]));
    }

    #[test]
    fn a_slot_stores_the_bytes_its_action_text_cannot_spell() {
        // The G502 Hero's factory slots, the G502 X Lightspeed's receiver profiles, and
        // the wired G502 X's: all three name DPI shift, and each is written back as read.
        for raw in [
            [0x90, 0x07, 0xFF, 0xFF],
            [0x90, 0x07, 0xFF, 0x00],
            [0x90, 0x07, 0x00, 0x00],
        ] {
            let sector = sector_with_binding(4, raw);
            assert!(sector_crc_valid(&sector));
            let mut editor = ProfileEditor::new(&sector, &description()).expect("editor");
            editor.set_binding(
                Table::Buttons,
                4,
                parse_action("dpi-shift").expect("parses"),
            );
            assert_eq!(editor.finish(), sector, "{raw:02X?}");
        }
    }

    #[test]
    fn a_slot_changes_action_without_losing_the_bytes_it_is_stored_with() {
        let sector = sector_with_binding(4, [0x90, 0x07, 0xFF, 0xFF]);
        let mut editor = ProfileEditor::new(&sector, &description()).expect("editor");
        editor.set_binding(
            Table::Buttons,
            6,
            Binding::Key {
                modifiers: 0x01,
                key: 0x17,
            },
        );
        let edited = editor.finish();

        let slot = BUTTON_OFFSET + 6 * BINDING_LEN;
        let mut expected: Vec<usize> = (slot..slot + BINDING_LEN).collect();
        expected.extend(checksum(sector.len()));
        assert_eq!(changed_offsets(&sector, &edited), expected);
        assert!(sector_crc_valid(&edited));
        let profile = Profile::parse(&edited, &description()).expect("profile");
        assert_eq!(
            profile.buttons[4],
            Binding::Special {
                code: 0x07,
                action: Some(SpecialAction::ShiftDpi),
                reserved: 0xFF,
                profile: 0xFF,
            }
        );
    }

    #[test]
    fn a_different_action_replaces_the_bytes_a_slot_was_stored_with() {
        // Text carries no bytes of its own, so binding another action writes that
        // action's own encoding: the reserved and profile bytes go to 0.
        let sector = sector_with_binding(4, [0x90, 0x07, 0xFF, 0xFF]);
        let mut editor = ProfileEditor::new(&sector, &description()).expect("editor");
        editor.set_binding(Table::Buttons, 4, parse_action("dpi-up").expect("parses"));
        let edited = editor.finish();

        let slot = BUTTON_OFFSET + 4 * BINDING_LEN;
        let mut expected: Vec<usize> = (slot + 1..slot + BINDING_LEN).collect();
        expected.extend(checksum(sector.len()));
        assert_eq!(changed_offsets(&sector, &edited), expected);
        assert!(sector_crc_valid(&edited));
        let profile = Profile::parse(&edited, &description()).expect("profile");
        assert_eq!(
            profile.buttons[4],
            Binding::Special {
                code: 0x03,
                action: Some(SpecialAction::NextDpi),
                reserved: 0,
                profile: 0,
            }
        );
    }

    #[test]
    fn unchanged_edit_reproduces_the_sector() {
        let sector = fixture_hex("/onboard/sectors/0002");
        let profile = Profile::parse(&sector, &description()).expect("profile");
        let mut editor = ProfileEditor::new(&sector, &description()).expect("editor");
        editor.set_report_rate_ms(profile.report_rate_ms);
        editor.set_dpi_stages(profile.dpi_stages);
        for (slot, binding) in profile.buttons.iter().enumerate() {
            editor.set_binding(Table::Buttons, slot, *binding);
        }
        assert_eq!(editor.finish(), sector);
    }

    #[test]
    fn dpi_edit_touches_only_the_stage_bytes_and_crc() {
        let sector = fixture_hex("/onboard/sectors/0001");
        let mut editor = ProfileEditor::new(&sector, &description()).expect("editor");
        editor.set_dpi_stages([Some(400), Some(1200), Some(1600), None, Some(3200)]);
        let edited = editor.finish();

        assert_eq!(changed_offsets(&sector, &edited), [3, 4, 9, 10, 253, 254]);
        assert!(sector_crc_valid(&edited));
        let profile = Profile::parse(&edited, &description()).expect("profile");
        assert_eq!(
            profile.dpi_stages,
            [Some(400), Some(1200), Some(1600), None, Some(3200)]
        );
    }

    #[test]
    fn binding_edit_touches_only_its_slot_and_crc() {
        let sector = fixture_hex("/onboard/sectors/0001");
        let mut editor = ProfileEditor::new(&sector, &description()).expect("editor");
        let binding = Binding::Key {
            modifiers: 0x01,
            key: 0x06,
        };
        editor.set_binding(Table::Buttons, 6, binding);
        let edited = editor.finish();

        let slot_start = BUTTON_OFFSET + 6 * BINDING_LEN;
        let mut expected: Vec<usize> = (slot_start..slot_start + BINDING_LEN).collect();
        expected.extend([253, 254]);
        assert_eq!(changed_offsets(&sector, &edited), expected);
        let profile = Profile::parse(&edited, &description()).expect("profile");
        assert_eq!(profile.buttons[6], binding);
        assert_eq!(
            profile.buttons[4],
            Binding::Special {
                code: 0x07,
                action: Some(SpecialAction::ShiftDpi),
                reserved: 0,
                profile: 0
            }
        );
    }

    #[test]
    fn name_edit_touches_only_the_name_and_crc() {
        let sector = fixture_hex("/onboard/sectors/0003");
        let mut editor = ProfileEditor::new(&sector, &description()).expect("editor");
        editor.set_name(Some("Omalogi Test"));
        let named = editor.finish();

        let field = NAME_OFFSET..NAME_OFFSET + NAME_LEN;
        let changed = changed_offsets(&sector, &named);
        assert!(!changed.is_empty());
        assert!(
            changed
                .iter()
                .all(|at| field.contains(at) || [253, 254].contains(at)),
            "{changed:?}"
        );
        assert!(sector_crc_valid(&named));
        let profile = Profile::parse(&named, &description()).expect("profile");
        assert_eq!(profile.name.as_deref(), Some("Omalogi Test"));

        // Setting the same name again changes nothing; clearing it restores 0xFF.
        let mut editor = ProfileEditor::new(&named, &description()).expect("editor");
        editor.set_name(Some("Omalogi Test"));
        assert_eq!(editor.finish(), named);
        let mut editor = ProfileEditor::new(&named, &description()).expect("editor");
        editor.set_name(None);
        let cleared = editor.finish();
        assert!(cleared[field].iter().all(|&byte| byte == 0xFF));
        assert_eq!(
            Profile::parse(&cleared, &description())
                .expect("profile")
                .name,
            None
        );
    }

    #[test]
    fn enabling_a_profile_touches_only_its_flag_and_crc() {
        let directory = fixture_hex("/onboard/sectors/0000");
        let before = crate::onboard::format::parse_directory(&directory, 5);
        let edited = directory_with_enabled(&directory, 2, !before[2].enabled);

        let changed = changed_offsets(&directory, &edited);
        assert!(
            changed
                .iter()
                .all(|at| [2 * DIRECTORY_ENTRY_LEN + 2, 253, 254].contains(at)),
            "{changed:?}"
        );
        assert!(sector_crc_valid(&edited));
        let after = crate::onboard::format::parse_directory(&edited, 5);
        assert_eq!(after[2].enabled, !before[2].enabled);
        assert_eq!(after[2].sector, before[2].sector);
        assert_eq!(after[0], before[0]);
        assert_eq!(
            directory_with_enabled(&edited, 2, before[2].enabled),
            directory
        );
    }

    #[test]
    fn edits_every_libratbag_layout_but_refuses_others() {
        let mut description = description();
        let sector = fixture_hex("/onboard/sectors/0001");
        description.profile_format = 5;
        assert!(ProfileEditor::new(&sector, &description).is_ok());
        description.profile_format = 6;
        assert!(matches!(
            ProfileEditor::new(&sector, &description),
            Err(DecodeError::UnsupportedLayout { .. })
        ));
    }
}
