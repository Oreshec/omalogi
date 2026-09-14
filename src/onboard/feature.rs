//! The HID++ 2.0 `OnboardProfiles` feature (0x8100): read functions only.
//!
//! Function numbers follow libratbag `src/hidpp20.c` (`CMD_ONBOARD_PROFILES_*`).
//! Built on `openlogi-hidpp`'s public traits so it can move upstream unchanged.

use std::sync::Arc;

use hidpp::{
    channel::HidppChannel,
    feature::{CreatableFeature, Feature},
    nibble::U4,
    protocol::v20::{Hidpp20Error, Message, MessageHeader},
};
use serde::Serialize;
use thiserror::Error;

use super::format::{DecodeError, Description, READ_CHUNK, ROM_DIRECTORY_SECTOR};

const GET_DESCRIPTION: u8 = 0;
const GET_MODE: u8 = 2;
const SET_CURRENT_PROFILE: u8 = 3;
const GET_CURRENT_PROFILE: u8 = 4;
const MEMORY_READ: u8 = 5;
const MEMORY_WRITE_START: u8 = 6;
const MEMORY_WRITE: u8 = 7;
const MEMORY_WRITE_END: u8 = 8;

const SHORT_PARAMS: usize = 3;
const LONG_PARAMS: usize = 16;

#[derive(Debug, Error)]
pub enum OnboardError {
    #[error("onboard profiles request failed")]
    Device(#[from] Hidpp20Error),
    #[error("onboard profiles data could not be decoded")]
    Decode(#[from] DecodeError),
    #[error("sector {0:#06x} holds factory profiles and is never written")]
    ProtectedSector(u16),
}

/// Who is in control of DPI, report rate and buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The stored profiles are active.
    Onboard,
    /// Host software settings are active.
    Host,
    Unknown(u8),
}

pub struct OnboardProfilesFeature {
    chan: Arc<HidppChannel>,
    device_index: u8,
    feature_index: u8,
}

impl Feature for OnboardProfilesFeature {}

impl CreatableFeature for OnboardProfilesFeature {
    const ID: u16 = 0x8100;
    const STARTING_VERSION: u8 = 0;

    fn new(chan: Arc<HidppChannel>, device_index: u8, feature_index: u8) -> Self {
        Self {
            chan,
            device_index,
            feature_index,
        }
    }
}

impl OnboardProfilesFeature {
    pub async fn description(&self) -> Result<Description, OnboardError> {
        let payload = self.call(GET_DESCRIPTION, &[]).await?;
        Ok(Description::parse(&payload)?)
    }

    pub async fn mode(&self) -> Result<Mode, Hidpp20Error> {
        Ok(match self.call(GET_MODE, &[]).await?[0] {
            1 => Mode::Onboard,
            2 => Mode::Host,
            other => Mode::Unknown(other),
        })
    }

    /// The raw active-profile index; see [`super::format::current_profile_position`].
    pub async fn current_profile_index(&self) -> Result<u8, Hidpp20Error> {
        Ok(self.call(GET_CURRENT_PROFILE, &[]).await?[1])
    }

    /// Selects the active onboard profile. `index` is 1-based like
    /// [`Self::current_profile_index`]; libratbag sends `index + 1` for its 0-based index.
    /// Nothing is written to profile memory.
    pub async fn set_current_profile(&self, index: u8) -> Result<(), Hidpp20Error> {
        self.call(SET_CURRENT_PROFILE, &[0x00, index]).await?;
        Ok(())
    }

    /// Reads a whole sector, 16 bytes per request.
    ///
    /// A read never crosses the end of the sector: the final request is
    /// re-aligned to `size - 16`, as libratbag does.
    pub async fn read_sector(&self, sector: u16, size: u16) -> Result<Vec<u8>, OnboardError> {
        let size = usize::from(size);
        if size < READ_CHUNK {
            return Err(DecodeError::TooShort {
                expected: READ_CHUNK,
                actual: size,
            }
            .into());
        }
        let mut data = vec![0; size];
        let mut offset = 0;
        while offset < size {
            let at = offset.min(size - READ_CHUNK);
            let [sector_hi, sector_lo] = sector.to_be_bytes();
            let [offset_hi, offset_lo] = u16::try_from(at)
                .expect("offsets stay below a u16 sector size")
                .to_be_bytes();
            let chunk = self
                .call(MEMORY_READ, &[sector_hi, sector_lo, offset_hi, offset_lo])
                .await?;
            data[at..at + READ_CHUNK].copy_from_slice(&chunk[..READ_CHUNK]);
            offset = at + READ_CHUNK;
        }
        Ok(data)
    }

    /// Writes a whole user sector to flash: `memoryAddrWrite(sector, 0, size)`, the data
    /// in 16-byte `memoryWrite` chunks (the last padded with 0xFF), then `memoryWriteEnd`.
    /// This is the sequence libratbag's `hidpp20_onboard_profiles_write_sector` uses.
    ///
    /// `data` must already end in its CRC. Factory sectors are refused before
    /// anything is sent. Callers read the sector back to verify the write.
    pub async fn write_sector(&self, sector: u16, data: &[u8]) -> Result<(), OnboardError> {
        if sector >= ROM_DIRECTORY_SECTOR {
            return Err(OnboardError::ProtectedSector(sector));
        }
        if data.len() < 2 {
            return Err(DecodeError::TooShort {
                expected: 2,
                actual: data.len(),
            }
            .into());
        }
        let size =
            u16::try_from(data.len()).map_err(|_| DecodeError::UnsupportedSectorSize(u16::MAX))?;
        let [sector_hi, sector_lo] = sector.to_be_bytes();
        let [size_hi, size_lo] = size.to_be_bytes();
        self.call(
            MEMORY_WRITE_START,
            &[sector_hi, sector_lo, 0, 0, size_hi, size_lo],
        )
        .await?;
        for chunk in data.chunks(LONG_PARAMS) {
            let mut padded = [0xFF; LONG_PARAMS];
            padded[..chunk.len()].copy_from_slice(chunk);
            self.call(MEMORY_WRITE, &padded).await?;
        }
        self.call(MEMORY_WRITE_END, &[]).await?;
        Ok(())
    }

    async fn call(&self, function: u8, params: &[u8]) -> Result<[u8; LONG_PARAMS], Hidpp20Error> {
        let header = MessageHeader {
            device_index: self.device_index,
            feature_index: self.feature_index,
            function_id: U4::from_lo(function),
            software_id: self.chan.get_sw_id(),
        };
        let message = if params.len() <= SHORT_PARAMS {
            let mut payload = [0; SHORT_PARAMS];
            payload[..params.len()].copy_from_slice(params);
            Message::Short(header, payload)
        } else {
            let mut payload = [0; LONG_PARAMS];
            payload[..params.len()].copy_from_slice(params);
            Message::Long(header, payload)
        };
        Ok(self.chan.send_v20(message).await?.extend_payload())
    }
}
