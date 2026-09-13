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

use super::format::{DecodeError, Description, READ_CHUNK};

const GET_DESCRIPTION: u8 = 0;
const GET_MODE: u8 = 2;
const GET_CURRENT_PROFILE: u8 = 4;
const MEMORY_READ: u8 = 5;

const SHORT_PARAMS: usize = 3;
const LONG_PARAMS: usize = 16;

#[derive(Debug, Error)]
pub enum OnboardError {
    #[error("onboard profiles request failed")]
    Device(#[from] Hidpp20Error),
    #[error("onboard profiles data could not be decoded")]
    Decode(#[from] DecodeError),
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
