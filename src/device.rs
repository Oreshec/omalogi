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
        Mode, OnboardError, OnboardProfilesFeature,
        format::{self, Description, DirectoryEntry, Profile},
    },
};

/// Device index of a device connected directly over USB rather than through a receiver.
const DIRECT_DEVICE_INDEX: u8 = 0xFF;

/// HID++ software id carried by every Omalogi request.
///
/// OpenLogi's agent leases the lowest free ids from 1 upward, one per open channel,
/// so the highest id keeps the two programs' replies apart while both use the device.
const SOFTWARE_ID: u8 = 0x0F;

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
    pub async fn open() -> Result<Self, SessionError> {
        let node = hidraw::find_supported()?;
        let path = node.path.display().to_string();
        let model = node.device;
        let raw = HidrawChannel::open(node)?;
        Self::connect(raw, model, path).await
    }

    /// Starts a session over any HID++ transport, such as an emulated device in tests.
    pub async fn connect(
        raw: impl RawHidChannel,
        model: SupportedDevice,
        path: String,
    ) -> Result<Self, SessionError> {
        let mut chan = HidppChannel::from_raw_channel(raw)
            .await
            .map_err(|source| SessionError::Channel {
                path: path.clone(),
                source,
            })?;
        let id = RequestSwId::new(U4::from_lo(SOFTWARE_ID)).expect("software id is non-zero");
        chan.set_sw_id_policy(SwIdPolicy::Fixed(id));
        let device = Device::new(Arc::new(chan), DIRECT_DEVICE_INDEX).await?;
        Ok(Self {
            device,
            model,
            path,
        })
    }

    async fn feature<F: CreatableFeature>(
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
            profiles.push(ProfileSlot {
                position,
                sector: entry.sector,
                enabled: entry.enabled,
                active: active_position == Some(position),
                crc_valid: format::sector_crc_valid(&sector),
                profile: Profile::parse(&sector, &description).map_err(OnboardError::from)?,
            });
        }

        Ok(OnboardState {
            mode,
            description,
            active_position,
            profiles,
        })
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
            let data = feature
                .read_sector(directory, description.sector_size)
                .await?;
            let entries = format::parse_directory(&data, max_entries.into());
            sectors.insert(directory, data);
            for entry in entries {
                if sectors.len() >= usize::from(description.sector_count) {
                    break;
                }
                if let Entry::Vacant(slot) = sectors.entry(entry.sector) {
                    slot.insert(
                        feature
                            .read_sector(entry.sector, description.sector_size)
                            .await?,
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

    async fn onboard_feature(&mut self) -> Result<Arc<OnboardProfilesFeature>, SessionError> {
        self.feature::<OnboardProfilesFeature>("onboard profiles (0x8100)")
            .await
    }
}

/// Reads the user profile directory, refusing one whose checksum does not match.
async fn read_directory(
    feature: &OnboardProfilesFeature,
    description: &Description,
) -> Result<Vec<DirectoryEntry>, SessionError> {
    let directory = feature
        .read_sector(format::USER_DIRECTORY_SECTOR, description.sector_size)
        .await?;
    if !format::sector_crc_valid(&directory) {
        return Err(SessionError::InvalidDirectoryChecksum);
    }
    Ok(format::parse_directory(
        &directory,
        description.profile_count.into(),
    ))
}

/// Expands an AdjustableDPI sensor list: plain values, and `min, 0xE000 | step, max`
/// ranges. A zero value ends the list.
fn expand_dpi_list(list: &[u16]) -> Vec<u16> {
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
fn report_rates_hz(bitmap: u8) -> Vec<u16> {
    (0..8u8)
        .rev()
        .filter(|bit| bitmap & (1 << bit) != 0)
        .filter_map(|bit| interval_to_hz(bit + 1))
        .collect()
}

fn interval_to_hz(interval_ms: u8) -> Option<u16> {
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
