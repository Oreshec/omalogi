//! A session with a connected device: the HID++ channel, feature lookup, reads and writes.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt::Write,
    sync::Arc,
};

use hidpp::{
    channel::{ChannelError, HidppChannel, RawHidChannel, RequestSwId, SwIdPolicy},
    device::{Device, DeviceError},
    feature::{
        CreatableFeature, adjustable_dpi::AdjustableDpiFeature,
        device_information::DeviceInformationFeature, report_rate::ReportRateFeature,
    },
    nibble::U4,
    protocol::v20::Hidpp20Error,
};
use serde::Serialize;
use thiserror::Error;

use crate::{
    hidraw::{self, HidrawChannel, HidrawError, SupportedDevice},
    onboard::{
        Mode, OnboardError, OnboardProfilesFeature, action,
        format::{self, Binding, Description, DirectoryEntry, Profile},
        label,
    },
};

/// Device index of a device connected directly over USB rather than through a receiver.
const DIRECT_DEVICE_INDEX: u8 = 0xFF;

/// HID++ software id for one-shot CLI commands.
///
/// Replies are matched by software id, and every process that opens the device
/// sees every reply. OpenLogi's agent leases the lowest free ids from 1 upward, one
/// per open channel, so Omalogi uses the highest ids, a different one per process kind.
pub const CLI_SOFTWARE_ID: u8 = 0x0F;
/// HID++ software id for the long-running daemon.
pub const DAEMON_SOFTWARE_ID: u8 = 0x0E;

/// Marks a `min, 0xE000 | step, max` range inside an AdjustableDPI sensor list.
const DPI_RANGE_MARKER: u16 = 0xE000;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error(transparent)]
    Hidraw(#[from] HidrawError),
    #[error("could not start a HID++ channel on {path}")]
    Channel {
        path: String,
        #[source]
        source: ChannelError,
    },
    #[error("HID++ software id {0} is outside 1..=15")]
    InvalidSoftwareId(u8),
    #[error("the device did not answer as a HID++ 2.0 device")]
    Device(#[from] DeviceError),
    #[error("the device does not report the {0} feature")]
    MissingFeature(&'static str),
    #[error("device request failed")]
    Request(#[from] Hidpp20Error),
    #[error(transparent)]
    Onboard(#[from] OnboardError),
    #[error(
        "the onboard profile directory has an invalid checksum; \
         the device may never have had profiles written"
    )]
    InvalidDirectoryChecksum,
    #[error("sector {sector:#06x} read back with an invalid checksum twice")]
    CorruptSector { sector: u16 },
    #[error("profile {number} does not exist; the device has {count} profile slots")]
    NoSuchProfile { number: usize, count: usize },
    #[error("profile {0} is disabled")]
    ProfileDisabled(usize),
    #[error("profiles can only be switched in onboard mode; the device is in {0:?} mode")]
    NotOnboardMode(Mode),
    #[error("the device did not switch to profile {requested}; it reports profile {reported:?}")]
    SwitchNotApplied {
        requested: usize,
        reported: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Firmware {
    pub kind: String,
    pub version: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub name: &'static str,
    pub vendor_id: u16,
    pub product_id: u16,
    /// Where the device was opened, e.g. `/dev/hidraw8`.
    pub path: String,
    pub firmware: Vec<Firmware>,
    pub dpi: u16,
    /// Every DPI value the sensor accepts.
    pub dpi_values: Vec<u16>,
    pub report_rate_hz: Option<u16>,
    pub report_rates_hz: Vec<u16>,
    pub onboard_mode: Mode,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileSlot {
    /// Position in the profile directory (0-based).
    pub position: usize,
    pub sector: u16,
    pub enabled: bool,
    pub active: bool,
    pub crc_valid: bool,
    pub profile: Profile,
    pub labels: BindingLabels,
    pub actions: BindingActions,
}

/// Each binding as the action text `profiles edit --button` accepts, aligned with its
/// slots; `None` for bindings that cannot be typed, such as macros.
#[derive(Debug, Clone, Serialize)]
pub struct BindingActions {
    pub buttons: Vec<Option<String>>,
    pub gshift_buttons: Vec<Option<String>>,
}

/// Readable names for a profile's bindings, aligned with its slots; `None` for unbound slots.
#[derive(Debug, Clone, Serialize)]
pub struct BindingLabels {
    pub buttons: Vec<Option<String>>,
    pub gshift_buttons: Vec<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OnboardState {
    pub mode: Mode,
    pub description: Description,
    pub active_position: Option<usize>,
    pub profiles: Vec<ProfileSlot>,
}

/// Onboard profile memory as read from the device, for restoring later.
#[derive(Debug, Clone, Serialize)]
pub struct Backup {
    pub backup_format: u32,
    pub device: &'static str,
    pub vendor_id: u16,
    pub product_id: u16,
    pub firmware: Vec<Firmware>,
    pub description: Description,
    /// Sector number (`"0001"`) to raw sector bytes as lowercase hex.
    pub sectors: BTreeMap<String, String>,
}

pub struct Session {
    device: Device,
    model: SupportedDevice,
    path: String,
}

impl Session {
    /// Opens the first connected supported device. Must be called inside a Tokio runtime.
    ///
    /// `software_id` is [`CLI_SOFTWARE_ID`] or [`DAEMON_SOFTWARE_ID`].
    pub async fn open(software_id: u8) -> Result<Self, SessionError> {
        let node = hidraw::find_supported()?;
        let path = node.path.display().to_string();
        let model = node.device;
        let raw = HidrawChannel::open(node)?;
        Self::connect(raw, model, path, software_id).await
    }

    /// Starts a session over any HID++ transport, such as an emulated device in tests.
    pub async fn connect(
        raw: impl RawHidChannel,
        model: SupportedDevice,
        path: String,
        software_id: u8,
    ) -> Result<Self, SessionError> {
        let id = (software_id <= 0x0F)
            .then(|| RequestSwId::new(U4::from_lo(software_id)))
            .flatten()
            .ok_or(SessionError::InvalidSoftwareId(software_id))?;
        let mut chan = HidppChannel::from_raw_channel(raw)
            .await
            .map_err(|source| SessionError::Channel {
                path: path.clone(),
                source,
            })?;
        chan.set_sw_id_policy(SwIdPolicy::Fixed(id));
        let device = Device::new(Arc::new(chan), DIRECT_DEVICE_INDEX).await?;
        Ok(Self {
            device,
            model,
            path,
        })
    }

    #[must_use]
    pub fn model(&self) -> SupportedDevice {
        self.model
    }

    pub(crate) async fn feature<F: CreatableFeature>(
        &mut self,
        name: &'static str,
    ) -> Result<Arc<F>, SessionError> {
        if let Some(feature) = self.device.get_feature::<F>() {
            return Ok(feature);
        }
        let info = self
            .device
            .root()
            .get_feature(F::ID)
            .await?
            .ok_or(SessionError::MissingFeature(name))?;
        Ok(self.device.add_feature::<F>(info.index))
    }

    pub async fn firmware(&mut self) -> Result<Vec<Firmware>, SessionError> {
        let feature = self
            .feature::<DeviceInformationFeature>("device information (0x0003)")
            .await?;
        let count = feature.get_device_info().await?.entity_count;
        let mut firmware = Vec::with_capacity(count.into());
        for entity in 0..count {
            let info = feature.get_fw_info(entity).await?;
            firmware.push(Firmware {
                kind: format!("{:?}", info.entity_type),
                // `openlogi-hidpp` already decodes these from packed BCD.
                version: format!(
                    "{} {:02}.{:02}.B{:04}",
                    info.firmware_prefix.trim(),
                    info.firmware_number,
                    info.revision,
                    info.build
                ),
                active: info.active,
            });
        }
        Ok(firmware)
    }

    pub async fn info(&mut self) -> Result<Info, SessionError> {
        let firmware = self.firmware().await?;

        let dpi = self
            .feature::<AdjustableDpiFeature>("adjustable DPI (0x2201)")
            .await?;
        let current_dpi = dpi.get_sensor_dpi(0).await?;
        let dpi_values = expand_dpi_list(&dpi.get_sensor_dpi_list(0).await?);

        let rate = self
            .feature::<ReportRateFeature>("report rate (0x8060)")
            .await?;
        let report_rate_hz = interval_to_hz(rate.get_report_rate().await?);
        let report_rates_hz = report_rates_hz(rate.get_report_rate_list().await?.bits());

        let onboard_mode = self.onboard_feature().await?.mode().await?;

        Ok(Info {
            name: self.model.name,
            vendor_id: self.model.vendor_id,
            product_id: self.model.product_id,
            path: self.path.clone(),
            firmware,
            dpi: current_dpi,
            dpi_values,
            report_rate_hz,
            report_rates_hz,
            onboard_mode,
        })
    }

    /// The sensor's DPI right now, which may differ from the profile's stages.
    pub async fn live_dpi(&mut self) -> Result<u16, SessionError> {
        let dpi = self
            .feature::<AdjustableDpiFeature>("adjustable DPI (0x2201)")
            .await?;
        Ok(dpi.get_sensor_dpi(0).await?)
    }

    /// Sets the sensor's DPI in the mouse's working memory, without writing any onboard
    /// profile, and returns the DPI the mouse reports afterwards.
    pub async fn set_live_dpi(&mut self, value: u16) -> Result<u16, SessionError> {
        let dpi = self
            .feature::<AdjustableDpiFeature>("adjustable DPI (0x2201)")
            .await?;
        dpi.set_sensor_dpi(0, value).await?;
        Ok(dpi.get_sensor_dpi(0).await?)
    }

    pub async fn onboard(&mut self) -> Result<OnboardState, SessionError> {
        let feature = self.onboard_feature().await?;
        let description = feature.description().await?;
        let mode = feature.mode().await?;
        let active_position =
            format::current_profile_position(feature.current_profile_index().await?);

        let entries = read_directory(&feature, &description).await?;
        let mut profiles = Vec::with_capacity(entries.len());
        for (position, entry) in entries.into_iter().enumerate() {
            let sector = feature
                .read_sector(entry.sector, description.sector_size)
                .await?;
            profiles.push(profile_slot_from(
                position,
                entry,
                &sector,
                &description,
                active_position,
            )?);
        }

        Ok(OnboardState {
            mode,
            description,
            active_position,
            profiles,
        })
    }

    /// A profile from memory that was just written and verified, without reading the
    /// mouse again: `data` is what the mouse now holds for profile `number`.
    pub async fn written_slot(
        &mut self,
        number: usize,
        entry: format::DirectoryEntry,
        active: bool,
        data: &[u8],
    ) -> Result<ProfileSlot, SessionError> {
        let description = self.onboard_feature().await?.description().await?;
        let position = number.saturating_sub(1);
        profile_slot_from(
            position,
            entry,
            data,
            &description,
            active.then_some(position),
        )
    }

    /// One profile as read from the mouse; `number` is 1-based, as shown to users.
    pub async fn profile_slot(&mut self, number: usize) -> Result<ProfileSlot, SessionError> {
        let feature = self.onboard_feature().await?;
        let description = feature.description().await?;
        let active_position =
            format::current_profile_position(feature.current_profile_index().await?);
        let entries = read_directory(&feature, &description).await?;
        let Some(position) = number
            .checked_sub(1)
            .filter(|&position| position < entries.len())
        else {
            return Err(SessionError::NoSuchProfile {
                number,
                count: entries.len(),
            });
        };
        let entry = entries[position];
        let sector = feature
            .read_sector(entry.sector, description.sector_size)
            .await?;
        profile_slot_from(position, entry, &sector, &description, active_position)
    }

    /// Makes an enabled profile active, then reads the active profile back to confirm.
    ///
    /// `number` is 1-based, as shown to users. Only the active-profile selection
    /// changes; profile memory is not written.
    pub async fn activate_profile(&mut self, number: usize) -> Result<(), SessionError> {
        let feature = self.onboard_feature().await?;
        let mode = feature.mode().await?;
        if mode != Mode::Onboard {
            return Err(SessionError::NotOnboardMode(mode));
        }

        let description = feature.description().await?;
        let entries = read_directory(&feature, &description).await?;
        let no_such_profile = SessionError::NoSuchProfile {
            number,
            count: entries.len(),
        };
        let Some(entry) = number
            .checked_sub(1)
            .and_then(|position| entries.get(position))
        else {
            return Err(no_such_profile);
        };
        if !entry.enabled {
            return Err(SessionError::ProfileDisabled(number));
        }
        let Ok(index) = u8::try_from(number) else {
            return Err(no_such_profile);
        };

        feature.set_current_profile(index).await?;

        let reported = format::current_profile_position(feature.current_profile_index().await?)
            .map(|position| position + 1);
        if reported != Some(number) {
            return Err(SessionError::SwitchNotApplied {
                requested: number,
                reported,
            });
        }
        Ok(())
    }

    /// The active profile number (1-based), with a single request.
    pub async fn active_profile(&mut self) -> Result<Option<usize>, SessionError> {
        let index = self
            .onboard_feature()
            .await?
            .current_profile_index()
            .await?;
        Ok(format::current_profile_position(index).map(|position| position + 1))
    }

    /// Reads the user and ROM profile directories and every sector they list.
    pub async fn backup(&mut self) -> Result<Backup, SessionError> {
        let firmware = self.firmware().await?;
        let feature = self.onboard_feature().await?;
        let description = feature.description().await?;

        let mut directories = vec![(format::USER_DIRECTORY_SECTOR, description.profile_count)];
        if description.rom_profile_count > 0 {
            directories.push((format::ROM_DIRECTORY_SECTOR, description.rom_profile_count));
        }

        let mut sectors = BTreeMap::new();
        for (directory, max_entries) in directories {
            let data = read_backup_sector(&feature, directory, description.sector_size).await?;
            let entries = format::parse_directory(&data, max_entries.into());
            sectors.insert(directory, data);
            for entry in entries {
                if sectors.len() >= usize::from(description.sector_count) {
                    break;
                }
                if let Entry::Vacant(slot) = sectors.entry(entry.sector) {
                    slot.insert(
                        read_backup_sector(&feature, entry.sector, description.sector_size).await?,
                    );
                }
            }
        }

        Ok(Backup {
            backup_format: 1,
            device: self.model.name,
            vendor_id: self.model.vendor_id,
            product_id: self.model.product_id,
            firmware,
            description,
            sectors: sectors
                .into_iter()
                .map(|(sector, data)| (format!("{sector:04x}"), to_hex(&data)))
                .collect(),
        })
    }

    pub(crate) async fn onboard_feature(
        &mut self,
    ) -> Result<Arc<OnboardProfilesFeature>, SessionError> {
        self.feature::<OnboardProfilesFeature>("onboard profiles (0x8100)")
            .await
    }
}

fn profile_slot_from(
    position: usize,
    entry: format::DirectoryEntry,
    sector: &[u8],
    description: &Description,
    active_position: Option<usize>,
) -> Result<ProfileSlot, SessionError> {
    let profile = Profile::parse(sector, description).map_err(OnboardError::from)?;
    Ok(ProfileSlot {
        position,
        sector: entry.sector,
        enabled: entry.enabled,
        active: active_position == Some(position),
        crc_valid: format::sector_crc_valid(sector),
        labels: BindingLabels {
            buttons: labels_for(&profile.buttons),
            gshift_buttons: labels_for(&profile.gshift_buttons),
        },
        actions: BindingActions {
            buttons: profile.buttons.iter().map(action::action_text).collect(),
            gshift_buttons: profile
                .gshift_buttons
                .iter()
                .map(action::action_text)
                .collect(),
        },
        profile,
    })
}

fn labels_for(bindings: &[Binding]) -> Vec<Option<String>> {
    bindings
        .iter()
        .map(|binding| (*binding != Binding::Disabled).then(|| label::binding(binding)))
        .collect()
}

/// Reads a sector for a backup. User sectors must pass their checksum, reading once more
/// if not, so a backup never holds bytes a restore would refuse. Factory sectors carry
/// no checksum.
async fn read_backup_sector(
    feature: &OnboardProfilesFeature,
    sector: u16,
    size: u16,
) -> Result<Vec<u8>, SessionError> {
    let data = feature.read_sector(sector, size).await?;
    if sector >= format::ROM_DIRECTORY_SECTOR || format::sector_crc_valid(&data) {
        return Ok(data);
    }
    let data = feature.read_sector(sector, size).await?;
    if format::sector_crc_valid(&data) {
        Ok(data)
    } else if sector == format::USER_DIRECTORY_SECTOR {
        Err(SessionError::InvalidDirectoryChecksum)
    } else {
        Err(SessionError::CorruptSector { sector })
    }
}

/// Reads the user profile directory, refusing one whose checksum does not match.
pub(crate) async fn read_directory(
    feature: &OnboardProfilesFeature,
    description: &Description,
) -> Result<Vec<DirectoryEntry>, SessionError> {
    // A read that overlaps other traffic can come back wrong, so read once more before
    // calling the directory invalid.
    for _ in 0..2 {
        let directory = feature
            .read_sector(format::USER_DIRECTORY_SECTOR, description.sector_size)
            .await?;
        if format::sector_crc_valid(&directory) {
            return Ok(format::parse_directory(
                &directory,
                description.profile_count.into(),
            ));
        }
    }
    Err(SessionError::InvalidDirectoryChecksum)
}

/// Expands an AdjustableDPI sensor list: plain values, and `min, 0xE000 | step, max`
/// ranges. A zero value ends the list.
pub(crate) fn expand_dpi_list(list: &[u16]) -> Vec<u16> {
    let mut values: Vec<u16> = Vec::new();
    let mut items = list.iter().copied().take_while(|&value| value != 0);
    while let Some(value) = items.next() {
        if value < DPI_RANGE_MARKER {
            values.push(value);
            continue;
        }
        let step = u32::from(value & !DPI_RANGE_MARKER);
        let (Some(&start), Some(end)) = (values.last(), items.next()) else {
            break;
        };
        if step == 0 {
            continue;
        }
        let mut dpi = u32::from(start) + step;
        while dpi <= u32::from(end) {
            values.push(u16::try_from(dpi).expect("bounded by a u16 end value"));
            dpi += step;
        }
    }
    values
}

/// Report rates in ascending order from a ReportRate bitmap, where bit `i` means an
/// `i + 1` ms interval.
pub(crate) fn report_rates_hz(bitmap: u8) -> Vec<u16> {
    (0..8u8)
        .rev()
        .filter(|bit| bitmap & (1 << bit) != 0)
        .filter_map(|bit| interval_to_hz(bit + 1))
        .collect()
}

pub(crate) fn interval_to_hz(interval_ms: u8) -> Option<u16> {
    (interval_ms != 0).then(|| 1000 / u16::from(interval_ms))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
            hex
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_g502x_dpi_range() {
        // The raw list the G502 X reports: 100, step 50, 25600.
        let values = expand_dpi_list(&[100, 0xE032, 25600, 0]);
        assert_eq!(values.first(), Some(&100));
        assert_eq!(values.get(1), Some(&150));
        assert_eq!(values.last(), Some(&25600));
        assert_eq!(values.len(), 511);
    }

    #[test]
    fn keeps_plain_dpi_lists() {
        assert_eq!(
            expand_dpi_list(&[400, 800, 1600, 0, 3200]),
            [400, 800, 1600]
        );
    }

    #[test]
    fn ignores_malformed_ranges() {
        assert_eq!(expand_dpi_list(&[0xE032, 25600]), Vec::<u16>::new());
        assert_eq!(expand_dpi_list(&[100, 0xE032]), [100]);
    }

    #[test]
    fn decodes_g502x_report_rates() {
        // 0x8B: 1, 2, 4 and 8 ms.
        assert_eq!(report_rates_hz(0x8B), [125, 250, 500, 1000]);
        assert_eq!(interval_to_hz(0), None);
    }

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(to_hex(&[0x00, 0x0A, 0xFF]), "000aff");
    }
}
