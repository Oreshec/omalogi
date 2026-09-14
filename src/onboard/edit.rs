//! Editing onboard profile sectors in place.
//!
//! An edit patches only the bytes whose decoded value changes, in a sector read from
//! the device, then recomputes the CRC. Bytes this crate does not decode survive
//! unchanged, unlike rebuilding the sector from a struct.

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
            Self::Special { code, profile, .. } => [0x90, code, 0x00, profile],
            Self::Macro { raw } | Self::Unknown { raw } => raw,
            Self::Disabled => [0xFF; BINDING_LEN],
        }
    }
}

/// The two binding tables of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Buttons,
    GShift,
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
    /// # Panics
    ///
    /// When `slot` is 16 or higher; callers validate slots first.
    pub fn set_binding(&mut self, table: Table, slot: usize, binding: Binding) {
        assert!(slot < BUTTON_SLOTS, "binding slot {slot} out of range");
        let base = match table {
            Table::Buttons => BUTTON_OFFSET,
            Table::GShift => GSHIFT_BUTTON_OFFSET,
        };
        let at = base + slot * BINDING_LEN;
        let current = Binding::decode(
            self.data[at..at + BINDING_LEN]
                .try_into()
                .expect("slice has binding length"),
        );
        if current != binding {
            self.data[at..at + BINDING_LEN].copy_from_slice(&binding.encode());
        }
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
    use crate::onboard::format::{Profile, SpecialAction, sector_crc_valid};

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
