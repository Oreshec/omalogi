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
        Mode, OnboardError, OnboardProfilesFeature,
        edit::{MAX_NAME_LEN, ProfileEditor, Table, directory_with_enabled},
        format::{self, Binding, DPI_STAGE_COUNT, Description, DirectoryDamage, Profile},
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
    #[error(
        "sector {sector:#06x} read back with an invalid checksum twice, so it was not \
         changed; if this persists, restore a backup"
    )]
    CorruptSector { sector: u16 },
    #[error("the profile directory's checksum matches; there is nothing to repair")]
    DirectoryIntact,
    #[error(
        "the profile directory read back differently twice, so it was not repaired; close \
         other programs using the mouse and try again"
    )]
    DirectoryUnstable,
    #[error(
        "the profile directory cannot be rebuilt from its entries: {0}; restore a backup made \
         before it was damaged with `omalogi restore`"
    )]
    DirectoryUnrepairable(DirectoryDamage),
    #[error(
        "the profile directory was not repaired: profile {number} (sector {sector:#06x}) also \
         fails its checksum; restore a backup made before it was damaged with `omalogi restore`"
    )]
    DamagedProfile { number: usize, sector: u16 },
    #[error("`{0}` is not a usable profile name: use up to 47 printable ASCII characters")]
    InvalidName(String),
    #[error("profile {0} is in use; activate another profile before turning it off")]
    ActiveProfile(usize),
    #[error("profile {0} is the only profile turned on; turn another one on first")]
    LastEnabledProfile(usize),
    #[error("the data for profile {number} is not a valid profile for this mouse")]
    InvalidProfileSector { number: usize },
    #[error(
        "the {name} has not been tested with Omalogi yet; run `omalogi accept-untested` or \
         accept it in the overlay to edit it (every write is still backed up and verified)"
    )]
    NotAccepted { name: &'static str },
    #[error("could not record the acceptance in {path}")]
    SaveConsent {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("there is no state directory to record the acceptance in; set XDG_STATE_HOME")]
    NoStateDirectory,
    #[error(
        "the change was written and verified, but the mouse is left on profile {current}: \
         switching back to profile {profile} to load it failed"
    )]
    LeftOnOtherProfile {
        profile: usize,
        current: usize,
        #[source]
        source: SessionError,
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
    /// A new profile name; an empty name clears it.
    pub name: Option<String>,
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
    enabled: bool,
    #[serde(skip)]
    previous: Vec<u8>,
    #[serde(skip)]
    edited: Vec<u8>,
}

impl EditPlan {
    /// The profile's memory before the edit, to put back for undo.
    #[must_use]
    pub fn previous_sector(&self) -> &[u8] {
        &self.previous
    }

    /// The profile's memory after the edit.
    #[must_use]
    pub fn edited_sector(&self) -> &[u8] {
        &self.edited
    }

    /// The profile's directory entry, as read when planning.
    #[must_use]
    pub fn entry(&self) -> format::DirectoryEntry {
        format::DirectoryEntry {
            sector: self.sector,
            enabled: self.enabled,
        }
    }
}

/// When written profile memory reaches the mouse.
///
/// The firmware reads a profile's settings only when it switches to that profile.
/// Writing its memory, or selecting the profile that is already active, leaves the
/// loaded settings unchanged (verified on a G502 X, docs/hardware-tests.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TakesEffect {
    /// The active profile was loaded again, so the mouse uses the change now.
    Now,
    /// The change is not in the active profile; it applies once that profile is activated.
    WhenActivated,
    /// The active profile changed but could not be loaded again.
    NotLoaded { reason: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct WriteReport {
    #[serde(flatten)]
    pub plan: EditPlan,
    /// Profile memory as it was before the write.
    pub backup: PathBuf,
    pub takes_effect: TakesEffect,
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

/// A profile the repaired directory lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RepairedEntry {
    pub profile: usize,
    pub sector: u16,
    pub enabled: bool,
}

/// A profile directory rebuilt from its own entries, not yet written.
#[derive(Debug, Clone, Serialize)]
pub struct DirectoryRepair {
    pub sector: u16,
    pub profiles: Vec<RepairedEntry>,
    #[serde(skip)]
    previous: Vec<u8>,
    #[serde(skip)]
    repaired: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub sectors: Vec<u16>,
    /// Profile memory as it was before the restore.
    pub backup: PathBuf,
    pub takes_effect: TakesEffect,
}

/// A backup file written by [`save_backup`].
#[derive(Debug, Clone, Deserialize)]
pub struct BackupFile {
    pub backup_format: u32,
    pub vendor_id: u16,
    pub product_id: u16,
    pub description: Description,
    pub sectors: BTreeMap<String, String>,
    /// Sectors saved with an invalid checksum; never written back.
    #[serde(default)]
    pub invalid_checksums: Vec<String>,
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
            let saved = if self.invalid_checksums.contains(&format!("{sector:04x}")) {
                "was saved with an invalid checksum, so it is never written back"
            } else {
                "has an invalid checksum"
            };
            return Err(invalid(format!("sector {sector:04x} {saved}")));
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

        let previous = read_user_sector(&feature, entry.sector, description.sector_size).await?;
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

        if let Some(name) = &changes.name {
            let name = name.trim();
            if name.len() > MAX_NAME_LEN || !name.bytes().all(|byte| (0x20..=0x7E).contains(&byte))
            {
                return Err(EditError::InvalidName(name.to_owned()));
            }
            editor.set_name(Some(name));
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
            enabled: entry.enabled,
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
        self.ensure_writes_accepted().await?;
        let backup = self.backup().await?;
        save_backup(&backup, backup_path)?;
        let takes_effect = self.write_plan(&plan).await?;
        Ok(WriteReport {
            plan,
            backup: backup_path.to_owned(),
            takes_effect,
        })
    }

    /// Writes a plan from [`Session::plan_profile_changes`], verifies it, and loads it
    /// when it changed the active profile. The caller makes sure a backup exists first.
    pub async fn write_plan(&mut self, plan: &EditPlan) -> Result<TakesEffect, EditError> {
        self.ensure_writes_accepted().await?;
        let feature = self.onboard_feature().await?;
        write_verified(&feature, plan.sector, &plan.edited, &plan.previous).await?;
        self.load_written(&[plan.sector]).await
    }

    /// Writes whole profile memory into profile `number`, as undo does. The data must be
    /// a valid profile for this mouse; the write is verified and loaded when active.
    pub async fn write_profile_sector(
        &mut self,
        number: usize,
        data: &[u8],
    ) -> Result<TakesEffect, EditError> {
        self.ensure_writes_accepted().await?;
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
        if data.len() != usize::from(description.sector_size)
            || !format::sector_crc_valid(data)
            || Profile::parse(data, &description).is_err()
        {
            return Err(EditError::InvalidProfileSector { number });
        }
        let previous = read_user_sector(&feature, entry.sector, description.sector_size).await?;
        if previous == data {
            return Err(EditError::NoChanges);
        }
        write_verified(&feature, entry.sector, data, &previous).await?;
        self.load_written(&[entry.sector]).await
    }

    /// Makes the mouse load the active profile again when `written` includes its sector.
    ///
    /// Selecting the active profile does nothing, so this switches to another enabled
    /// profile and straight back. Callers hold the device lock, which keeps the daemon
    /// from acting on the brief switch.
    async fn load_written(&mut self, written: &[u16]) -> Result<TakesEffect, EditError> {
        let feature = self.onboard_feature().await?;
        if feature.mode().await.map_err(SessionError::from)? != Mode::Onboard {
            // Onboard profiles are not in use at all in host mode.
            return Ok(TakesEffect::WhenActivated);
        }
        let description = feature.description().await?;
        let entries = read_directory(&feature, &description).await?;
        let index = feature
            .current_profile_index()
            .await
            .map_err(SessionError::from)?;
        let Some(active) = format::current_profile_position(index).filter(|&position| {
            entries
                .get(position)
                .is_some_and(|entry| written.contains(&entry.sector))
        }) else {
            return Ok(TakesEffect::WhenActivated);
        };
        let Some(other) = entries
            .iter()
            .enumerate()
            .find(|&(position, entry)| position != active && entry.enabled)
            .map(|(position, _)| position)
        else {
            return Ok(TakesEffect::NotLoaded {
                reason: "the mouse loads a profile when it switches to it, and no other \
                         profile is enabled to switch through"
                    .to_owned(),
            });
        };
        let (active, other) = (active + 1, other + 1);

        // The directory is known already, so switch directly instead of through
        // `activate_profile`, which would read it again for each switch.
        let away = switch_profile(&feature, other).await;
        let back = match switch_profile(&feature, active).await {
            Ok(()) => Ok(()),
            // Never leave the mouse on the other profile without trying again.
            Err(_) => switch_profile(&feature, active).await,
        };
        match (away, back) {
            (Ok(()), Ok(())) => Ok(TakesEffect::Now),
            (Err(error), Ok(())) => Ok(TakesEffect::NotLoaded {
                reason: format!(
                    "switching to profile {other} to reload it failed: {}",
                    crate::error_chain(&error)
                ),
            }),
            (_, Err(source)) => {
                let current = feature
                    .current_profile_index()
                    .await
                    .ok()
                    .and_then(format::current_profile_position)
                    .map_or(other, |position| position + 1);
                Err(EditError::LeftOnOtherProfile {
                    profile: active,
                    current,
                    source,
                })
            }
        }
    }

    /// Checks that profile `number` can be turned on or off, without writing.
    pub async fn check_profile_enabled(
        &mut self,
        number: usize,
        enabled: bool,
    ) -> Result<(), EditError> {
        self.plan_enabled(number, enabled).await.map(|_| ())
    }

    /// Turns profile `number` on or off in the profile directory, verified and rolled back
    /// on a mismatch. The profile in use, or the only one turned on, cannot be turned off.
    /// The caller makes sure a backup exists first.
    pub async fn set_profile_enabled(
        &mut self,
        number: usize,
        enabled: bool,
    ) -> Result<(), EditError> {
        let (previous, edited) = self.plan_enabled(number, enabled).await?;
        self.ensure_writes_accepted().await?;
        let feature = self.onboard_feature().await?;
        write_verified(&feature, format::USER_DIRECTORY_SECTOR, &edited, &previous).await
    }

    /// The directory as read, and with the profile's flag changed.
    async fn plan_enabled(
        &mut self,
        number: usize,
        enabled: bool,
    ) -> Result<(Vec<u8>, Vec<u8>), EditError> {
        let feature = self.onboard_feature().await?;
        let description = feature.description().await?;
        let previous = read_user_sector(
            &feature,
            format::USER_DIRECTORY_SECTOR,
            description.sector_size,
        )
        .await?;
        let entries = format::parse_directory(&previous, description.profile_count.into());
        let Some(position) = number
            .checked_sub(1)
            .filter(|&position| position < entries.len())
        else {
            return Err(EditError::NoSuchProfile {
                number,
                count: entries.len(),
            });
        };
        if entries[position].enabled == enabled {
            return Err(EditError::NoChanges);
        }
        if !enabled {
            let index = feature
                .current_profile_index()
                .await
                .map_err(SessionError::from)?;
            if format::current_profile_position(index) == Some(position) {
                return Err(EditError::ActiveProfile(number));
            }
            if entries.iter().filter(|entry| entry.enabled).count() <= 1 {
                return Err(EditError::LastEnabledProfile(number));
            }
        }
        let edited = directory_with_enabled(&previous, position, enabled);
        Ok((previous, edited))
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
            // The mouse's current bytes are only compared and, if a write fails, put back as
            // they were, so they need not pass their checksum: restoring is how a damaged
            // sector is fixed.
            let previous = read_sector_as_is(&feature, sector, description.sector_size).await?;
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
        let takes_effect = self.load_written(&plan.sectors).await?;
        Ok(RestoreReport {
            sectors: plan.sectors,
            backup: backup_path.to_owned(),
            takes_effect,
        })
    }

    /// Rebuilds a profile directory whose checksum does not match from its own entries,
    /// without writing. Refused unless the directory reads the same twice, its entries are
    /// consistent ([`format::directory_entries_for_repair`]) and every profile it lists
    /// passes its own checksum.
    pub async fn plan_directory_repair(&mut self) -> Result<DirectoryRepair, EditError> {
        let feature = self.onboard_feature().await?;
        let description = feature.description().await?;
        let size = description.sector_size;
        let first = feature
            .read_sector(format::USER_DIRECTORY_SECTOR, size)
            .await?;
        if format::sector_crc_valid(&first) {
            return Err(EditError::DirectoryIntact);
        }
        let second = feature
            .read_sector(format::USER_DIRECTORY_SECTOR, size)
            .await?;
        if format::sector_crc_valid(&second) {
            return Err(EditError::DirectoryIntact);
        }
        if first != second {
            return Err(EditError::DirectoryUnstable);
        }

        let entries =
            format::directory_entries_for_repair(&first, description.profile_count.into())
                .map_err(EditError::DirectoryUnrepairable)?;
        let mut profiles = Vec::with_capacity(entries.len());
        for (position, entry) in entries.iter().enumerate() {
            let number = position + 1;
            match read_user_sector(&feature, entry.sector, size).await {
                Ok(_) => {}
                Err(EditError::CorruptSector { sector }) => {
                    return Err(EditError::DamagedProfile { number, sector });
                }
                Err(error) => return Err(error),
            }
            profiles.push(RepairedEntry {
                profile: number,
                sector: entry.sector,
                enabled: entry.enabled,
            });
        }
        let repaired = format::rebuilt_directory(&first, entries.len());
        Ok(DirectoryRepair {
            sector: format::USER_DIRECTORY_SECTOR,
            profiles,
            previous: first,
            repaired,
        })
    }

    /// Writes the repaired directory, verified by reading it back. The caller makes sure a
    /// backup exists first.
    pub async fn repair_directory(&mut self) -> Result<DirectoryRepair, EditError> {
        let repair = self.plan_directory_repair().await?;
        self.ensure_writes_accepted().await?;
        let feature = self.onboard_feature().await?;
        write_verified(&feature, repair.sector, &repair.repaired, &repair.previous).await?;
        Ok(repair)
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

/// Reads a user sector, reading it once more if its checksum fails. Nothing is ever
/// edited or rolled back to from bytes that do not check out: a read that overlapped
/// another process's traffic has come back wrong on a G502 X.
async fn read_user_sector(
    feature: &OnboardProfilesFeature,
    sector: u16,
    size: u16,
) -> Result<Vec<u8>, EditError> {
    for _ in 0..2 {
        let data = feature.read_sector(sector, size).await?;
        if format::sector_crc_valid(&data) {
            return Ok(data);
        }
    }
    Err(EditError::CorruptSector { sector })
}

/// Reads a sector twice when its checksum fails, and returns the bytes whether or not the
/// second read passes.
async fn read_sector_as_is(
    feature: &OnboardProfilesFeature,
    sector: u16,
    size: u16,
) -> Result<Vec<u8>, EditError> {
    let data = feature.read_sector(sector, size).await?;
    if format::sector_crc_valid(&data) {
        return Ok(data);
    }
    Ok(feature.read_sector(sector, size).await?)
}

/// Selects profile `number` (1-based) and confirms that the mouse reports it.
async fn switch_profile(
    feature: &OnboardProfilesFeature,
    number: usize,
) -> Result<(), SessionError> {
    let index = u8::try_from(number).map_err(|_| SessionError::NoSuchProfile {
        number,
        count: usize::from(u8::MAX),
    })?;
    feature.set_current_profile(index).await?;
    let reported = format::current_profile_position(feature.current_profile_index().await?)
        .map(|position| position + 1);
    if reported == Some(number) {
        Ok(())
    } else {
        Err(SessionError::SwitchNotApplied {
            requested: number,
            reported,
        })
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
