//! Writing onboard profile memory: validated edits, automatic backups, verified writes,
//! and restore.
//!
//! Every write first saves a backup of all profile memory to a new file, then writes
//! one sector, reads it back and compares. When the read-back differs, or the write
//! fails part way, the previous bytes are written back and the error says whether
//! that worked. Factory (ROM) sectors are never written.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use hidpp::feature::{adjustable_dpi::AdjustableDpiFeature, report_rate::ReportRateFeature};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    device::{
        Backup, Session, SessionError, expand_dpi_list, interval_to_hz, read_directory,
        report_rates_hz,
    },
    onboard::{
        OnboardError, OnboardProfilesFeature,
        edit::{ProfileEditor, Table},
        format::{self, Binding, DPI_STAGE_COUNT, Description, Profile},
    },
};

const BACKUP_FORMAT: u32 = 1;

#[derive(Debug, Error)]
pub enum EditError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Onboard(#[from] OnboardError),
    #[error("profile {number} does not exist; the device has {count} profile slots")]
    NoSuchProfile { number: usize, count: usize },
    #[error("give between 1 and 5 DPI stages, not {0}")]
    DpiStageCount(usize),
    #[error("the sensor does not support {0} DPI")]
    DpiNotSupported(u16),
    #[error("{0} DPI is not one of the profile's DPI stages")]
    DpiNotAStage(u16),
    #[error("the profile's {which} DPI stage would no longer exist; pass --{which}-dpi")]
    StageNeeded { which: &'static str },
    #[error("the mouse does not support {hz} Hz; it supports {supported} Hz")]
    ReportRateNotSupported { hz: u16, supported: String },
    #[error("slot {slot} of the {table} table is not a button on this mouse")]
    SlotNotEditable { table: &'static str, slot: usize },
    #[error("the profile already has these settings; nothing was written")]
    NoChanges,
    #[error("the mouse already matches the backup; nothing was written")]
    AlreadyRestored,
    #[error("could not save a backup to {path}")]
    SaveBackup {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("could not read the backup {path}")]
    ReadBackup {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("{path} is not a usable Omalogi backup: {reason}")]
    InvalidBackup { path: String, reason: String },
    #[error("the backup does not match this mouse: {0}")]
    BackupMismatch(String),
    #[error(
        "sector {sector:#06x} read back differently after writing; {}",
        restore_outcome(.restored)
    )]
    VerifyFailed { sector: u16, restored: bool },
    #[error("writing sector {sector:#06x} failed; {}", restore_outcome(.restored))]
    WriteFailed {
        sector: u16,
        restored: bool,
        #[source]
        source: OnboardError,
    },
}

fn restore_outcome(restored: &bool) -> &'static str {
    if *restored {
        "its previous contents were written back"
    } else {
        "writing its previous contents back also failed; restore from the backup file"
    }
}

/// Changes to one profile. Unset fields keep their current values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileChanges {
    /// DPI stages in order, 1 to 5 of them.
    pub dpi_stages: Option<Vec<u16>>,
    /// The stage active after switching to the profile, by DPI value.
    pub default_dpi: Option<u16>,
    /// The stage held with the DPI shift button, by DPI value.
    pub shift_dpi: Option<u16>,
    pub report_rate_hz: Option<u16>,
    pub buttons: Vec<(usize, Binding)>,
    pub gshift_buttons: Vec<(usize, Binding)>,
}

impl ProfileChanges {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A validated edit, not yet written.
#[derive(Debug, Clone, Serialize)]
pub struct EditPlan {
    pub profile: usize,
    pub sector: u16,
    pub before: Profile,
    pub after: Profile,
    #[serde(skip)]
    previous: Vec<u8>,
    #[serde(skip)]
    edited: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WriteReport {
    #[serde(flatten)]
    pub plan: EditPlan,
    /// Profile memory as it was before the write.
    pub backup: PathBuf,
}

#[derive(Debug, Clone)]
struct SectorWrite {
    sector: u16,
    data: Vec<u8>,
    previous: Vec<u8>,
}

/// The sectors a restore would write, in write order.
#[derive(Debug, Clone, Serialize)]
pub struct RestorePlan {
    pub sectors: Vec<u16>,
    #[serde(skip)]
    writes: Vec<SectorWrite>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub sectors: Vec<u16>,
    /// Profile memory as it was before the restore.
    pub backup: PathBuf,
}

/// A backup file written by [`save_backup`].
#[derive(Debug, Clone, Deserialize)]
pub struct BackupFile {
    pub backup_format: u32,
    pub vendor_id: u16,
    pub product_id: u16,
    pub description: Description,
    pub sectors: BTreeMap<String, String>,
    #[serde(skip)]
    path: String,
}

impl BackupFile {
    pub fn load(path: &Path) -> Result<Self, EditError> {
        let display = path.display().to_string();
        let text = fs::read_to_string(path).map_err(|source| EditError::ReadBackup {
            path: display.clone(),
            source,
        })?;
        let mut file: Self =
            serde_json::from_str(&text).map_err(|error| EditError::InvalidBackup {
                path: display.clone(),
                reason: error.to_string(),
            })?;
        if file.backup_format != BACKUP_FORMAT {
            return Err(EditError::InvalidBackup {
                path: display,
                reason: format!("unsupported backup_format {}", file.backup_format),
            });
        }
        file.path = display;
        Ok(file)
    }

    /// A user sector from the backup, checked for size and CRC.
    fn user_sector(&self, sector: u16) -> Result<Vec<u8>, EditError> {
        let invalid = |reason: String| EditError::InvalidBackup {
            path: self.path.clone(),
            reason,
        };
        let hex = self
            .sectors
            .get(&format!("{sector:04x}"))
            .ok_or_else(|| invalid(format!("sector {sector:04x} is missing")))?;
        let data =
            from_hex(hex).ok_or_else(|| invalid(format!("sector {sector:04x} is not hex")))?;
        if data.len() != usize::from(self.description.sector_size) {
            return Err(invalid(format!("sector {sector:04x} has the wrong size")));
        }
        if !format::sector_crc_valid(&data) {
            return Err(invalid(format!(
                "sector {sector:04x} has an invalid checksum"
            )));
        }
        Ok(data)
    }
}

/// Saves a backup to a new file; an existing file is never overwritten.
pub fn save_backup(backup: &Backup, path: &Path) -> Result<(), EditError> {
    let error = |source| EditError::SaveBackup {
        path: path.display().to_string(),
        source,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(error)?;
    }
    let json = serde_json::to_string_pretty(backup).expect("backups always serialize") + "\n";
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(error)?;
    file.write_all(json.as_bytes()).map_err(error)?;
    file.sync_all().map_err(error)
}

impl Session {
    /// Validates `changes` against the mouse and returns the edited profile without writing.
    pub async fn plan_profile_changes(
        &mut self,
        number: usize,
        changes: &ProfileChanges,
    ) -> Result<EditPlan, EditError> {
        let feature = self.onboard_feature().await?;
        let description = feature.description().await?;
        let entries = read_directory(&feature, &description).await?;
        let entry = number
            .checked_sub(1)
            .and_then(|position| entries.get(position))
            .copied()
            .ok_or(EditError::NoSuchProfile {
                number,
                count: entries.len(),
            })?;
        if entry.sector >= format::ROM_DIRECTORY_SECTOR {
            return Err(OnboardError::ProtectedSector(entry.sector).into());
        }

        let previous = feature
            .read_sector(entry.sector, description.sector_size)
            .await?;
        let before = Profile::parse(&previous, &description).map_err(OnboardError::from)?;
        let mut editor = ProfileEditor::new(&previous, &description).map_err(OnboardError::from)?;

        let mut stages = before.dpi_stages;
        if let Some(new_stages) = &changes.dpi_stages {
            if !(1..=DPI_STAGE_COUNT).contains(&new_stages.len()) {
                return Err(EditError::DpiStageCount(new_stages.len()));
            }
            let supported = self.dpi_values().await?;
            if let Some(&dpi) = new_stages.iter().find(|dpi| !supported.contains(dpi)) {
                return Err(EditError::DpiNotSupported(dpi));
            }
            stages = std::array::from_fn(|stage| new_stages.get(stage).copied());
            editor.set_dpi_stages(stages);
        }
        let previous_dpi = |index: u8| before.dpi_stages.get(usize::from(index)).copied().flatten();
        if changes.dpi_stages.is_some() || changes.default_dpi.is_some() {
            let index = stage_index(
                &stages,
                changes.default_dpi,
                before.default_dpi_index,
                previous_dpi(before.default_dpi_index),
                "default",
            )?;
            editor.set_default_dpi_index(index);
        }
        if changes.dpi_stages.is_some() || changes.shift_dpi.is_some() {
            let index = stage_index(
                &stages,
                changes.shift_dpi,
                before.shift_dpi_index,
                previous_dpi(before.shift_dpi_index),
                "shift",
            )?;
            editor.set_shift_dpi_index(index);
        }

        if let Some(hz) = changes.report_rate_hz {
            let rate = self
                .feature::<ReportRateFeature>("report rate (0x8060)")
                .await?;
            let bitmap = rate
                .get_report_rate_list()
                .await
                .map_err(SessionError::from)?
                .bits();
            let interval = (1..=8u8)
                .find(|&ms| bitmap & (1 << (ms - 1)) != 0 && interval_to_hz(ms) == Some(hz))
                .ok_or_else(|| EditError::ReportRateNotSupported {
                    hz,
                    supported: report_rates_hz(bitmap)
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                })?;
            editor.set_report_rate_ms(interval);
        }

        let tables = [
            (Table::Buttons, "buttons", &changes.buttons, &before.buttons),
            (
                Table::GShift,
                "G-Shift",
                &changes.gshift_buttons,
                &before.gshift_buttons,
            ),
        ];
        for (table, name, edits, current) in tables {
            for &(slot, binding) in edits {
                // Physical buttons, plus slots the device already binds (the wheel).
                let editable = slot < current.len()
                    && (slot < usize::from(description.button_count)
                        || current[slot] != Binding::Disabled);
                if !editable {
                    return Err(EditError::SlotNotEditable { table: name, slot });
                }
                editor.set_binding(table, slot, binding);
            }
        }

        let edited = editor.finish();
        if edited == previous {
            return Err(EditError::NoChanges);
        }
        let after = Profile::parse(&edited, &description).map_err(OnboardError::from)?;
        Ok(EditPlan {
            profile: number,
            sector: entry.sector,
            before,
            after,
            previous,
            edited,
        })
    }

    /// Plans `changes`, backs up profile memory to `backup_path`, writes and verifies.
    pub async fn apply_profile_changes(
        &mut self,
        number: usize,
        changes: &ProfileChanges,
        backup_path: &Path,
    ) -> Result<WriteReport, EditError> {
        let plan = self.plan_profile_changes(number, changes).await?;
        let backup = self.backup().await?;
        save_backup(&backup, backup_path)?;
        let feature = self.onboard_feature().await?;
        write_verified(&feature, plan.sector, &plan.edited, &plan.previous).await?;
        Ok(WriteReport {
            plan,
            backup: backup_path.to_owned(),
        })
    }

    /// The user sectors that differ from `backup`, after checking it belongs to this mouse.
    pub async fn plan_restore(&mut self, backup: &BackupFile) -> Result<RestorePlan, EditError> {
        if (backup.vendor_id, backup.product_id)
            != (self.model().vendor_id, self.model().product_id)
        {
            return Err(EditError::BackupMismatch(format!(
                "it was made from {:04x}:{:04x}",
                backup.vendor_id, backup.product_id
            )));
        }
        let feature = self.onboard_feature().await?;
        let description = feature.description().await?;
        if backup.description != description {
            return Err(EditError::BackupMismatch(
                "its profile memory layout differs".to_owned(),
            ));
        }

        let directory = backup.user_sector(format::USER_DIRECTORY_SECTOR)?;
        let mut sectors: Vec<u16> =
            format::parse_directory(&directory, description.profile_count.into())
                .into_iter()
                .map(|entry| entry.sector)
                .filter(|&sector| {
                    sector != format::USER_DIRECTORY_SECTOR && sector < format::ROM_DIRECTORY_SECTOR
                })
                .collect();
        sectors.dedup();
        // Profiles first, then the directory that points at them.
        sectors.push(format::USER_DIRECTORY_SECTOR);

        let mut writes = Vec::new();
        for sector in sectors {
            let data = backup.user_sector(sector)?;
            let previous = feature.read_sector(sector, description.sector_size).await?;
            if data != previous {
                writes.push(SectorWrite {
                    sector,
                    data,
                    previous,
                });
            }
        }
        Ok(RestorePlan {
            sectors: writes.iter().map(|write| write.sector).collect(),
            writes,
        })
    }

    /// Restores user profile memory from `backup`, first backing up the current memory
    /// to `backup_path`.
    pub async fn restore(
        &mut self,
        backup: &BackupFile,
        backup_path: &Path,
    ) -> Result<RestoreReport, EditError> {
        let plan = self.plan_restore(backup).await?;
        if plan.writes.is_empty() {
            return Err(EditError::AlreadyRestored);
        }
        let current = self.backup().await?;
        save_backup(&current, backup_path)?;
        let feature = self.onboard_feature().await?;
        for write in &plan.writes {
            write_verified(&feature, write.sector, &write.data, &write.previous).await?;
        }
        Ok(RestoreReport {
            sectors: plan.sectors,
            backup: backup_path.to_owned(),
        })
    }

    async fn dpi_values(&mut self) -> Result<Vec<u16>, EditError> {
        let dpi = self
            .feature::<AdjustableDpiFeature>("adjustable DPI (0x2201)")
            .await?;
        let list = dpi
            .get_sensor_dpi_list(0)
            .await
            .map_err(SessionError::from)?;
        Ok(expand_dpi_list(&list))
    }
}

/// Where the default or shift stage points after an edit.
///
/// A requested DPI wins. Otherwise the stage keeps its previous DPI value wherever
/// that value now sits, so editing stages never silently changes it. When that value
/// is gone the caller must choose; only a stage that had no value keeps its index.
fn stage_index(
    stages: &[Option<u16>; DPI_STAGE_COUNT],
    requested: Option<u16>,
    current_index: u8,
    current_dpi: Option<u16>,
    which: &'static str,
) -> Result<u8, EditError> {
    let position = |dpi: u16| {
        stages
            .iter()
            .position(|stage| *stage == Some(dpi))
            .map(|index| u8::try_from(index).expect("at most five stages"))
    };
    match requested {
        Some(dpi) => position(dpi).ok_or(EditError::DpiNotAStage(dpi)),
        None => match current_dpi {
            Some(dpi) => position(dpi),
            None => stages
                .get(usize::from(current_index))
                .is_some_and(Option::is_some)
                .then_some(current_index),
        }
        .ok_or(EditError::StageNeeded { which }),
    }
}

async fn write_verified(
    feature: &OnboardProfilesFeature,
    sector: u16,
    data: &[u8],
    previous: &[u8],
) -> Result<(), EditError> {
    let size = u16::try_from(data.len()).expect("sector sizes fit in u16");
    if let Err(source) = feature.write_sector(sector, data).await {
        let restored = put_back(feature, sector, previous, size).await;
        return Err(EditError::WriteFailed {
            sector,
            restored,
            source,
        });
    }
    let read_back = feature.read_sector(sector, size).await?;
    if read_back != data {
        let restored = put_back(feature, sector, previous, size).await;
        return Err(EditError::VerifyFailed { sector, restored });
    }
    Ok(())
}

/// Writes `previous` back and confirms it; true when the sector is as it was.
async fn put_back(
    feature: &OnboardProfilesFeature,
    sector: u16,
    previous: &[u8],
    size: u16,
) -> bool {
    feature.write_sector(sector, previous).await.is_ok()
        && feature
            .read_sector(sector, size)
            .await
            .is_ok_and(|data| data == previous)
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_index_follows_dpi_values() {
        let stages = [Some(400), Some(800), Some(1600), None, None];
        // A requested DPI wins.
        assert_eq!(
            stage_index(&stages, Some(1600), 0, Some(800), "default").ok(),
            Some(2)
        );
        assert!(matches!(
            stage_index(&stages, Some(1200), 0, None, "default"),
            Err(EditError::DpiNotAStage(1200))
        ));
        // Otherwise the previous DPI value is kept at its new position: shift 800 moves 0 -> 1.
        assert_eq!(
            stage_index(&stages, None, 0, Some(800), "shift").ok(),
            Some(1)
        );
        // A value that no longer exists is never replaced silently.
        assert!(matches!(
            stage_index(&stages, None, 2, Some(2400), "default"),
            Err(EditError::StageNeeded { which: "default" })
        ));
        // A stage that had no value keeps its index while that index names a stage.
        assert_eq!(stage_index(&stages, None, 2, None, "shift").ok(), Some(2));
        assert!(matches!(
            stage_index(&stages, None, 3, None, "shift"),
            Err(EditError::StageNeeded { which: "shift" })
        ));
    }

    #[test]
    fn hex_decoding_rejects_garbage() {
        assert_eq!(from_hex("00ff"), Some(vec![0x00, 0xFF]));
        assert_eq!(from_hex("0"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn restore_outcomes_read_naturally() {
        let error = EditError::VerifyFailed {
            sector: 1,
            restored: true,
        };
        assert_eq!(
            error.to_string(),
            "sector 0x0001 read back differently after writing; its previous contents were written back"
        );
    }
}
