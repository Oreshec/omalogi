//! An emulated wired G502 X (046d:c099) for integration tests.
//!
//! It answers HID++ 2.0 requests with the bytes a real device returned
//! (`tests/fixtures/g502x-c099.json`), replies with HID++ error reports where a
//! device would, and keeps the onboard mode and active profile as state.

use std::{
    collections::HashMap,
    error::Error,
    sync::{Arc, Mutex},
};

use hidpp::{async_trait, channel::RawHidChannel};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

const FIXTURE: &str = include_str!("../fixtures/g502x-c099.json");

const LONG_REPORT_ID: u8 = 0x11;
const LONG_REPORT_LEN: usize = 20;
const ERROR_FEATURE_INDEX: u8 = 0xFF;
const MEMORY_READ_LEN: usize = 16;

// HID++ 2.0 error codes.
const ERR_INVALID_ARGUMENT: u8 = 0x02;
const ERR_INVALID_FEATURE_INDEX: u8 = 0x06;
const ERR_INVALID_FUNCTION_ID: u8 = 0x07;

type BoxError = Box<dyn Error + Sync + Send>;

/// Mutable device state, shared with the test for setup and assertions.
pub struct State {
    /// `getMode` value: 1 onboard, 2 host.
    pub mode: u8,
    /// 1-based active profile index.
    pub current_profile: u8,
    /// When set, `setCurrentProfile` succeeds without changing anything.
    pub ignore_profile_switch: bool,
    /// Every report the host wrote, in order.
    pub requests: Vec<Vec<u8>>,
}

struct Feature {
    index: u8,
    id: u16,
    version: u8,
}

struct Fixture {
    features: Vec<Feature>,
    device_info: Vec<u8>,
    fw_entities: Vec<Vec<u8>>,
    dpi_list: Vec<u8>,
    dpi_current: Vec<u8>,
    rate_list: Vec<u8>,
    rate_current: Vec<u8>,
    description: Vec<u8>,
    sectors: HashMap<u16, Vec<u8>>,
}

pub struct FakeG502x {
    fixture: Fixture,
    state: Arc<Mutex<State>>,
    responses: mpsc::UnboundedSender<Vec<u8>>,
    reports: AsyncMutex<mpsc::UnboundedReceiver<Vec<u8>>>,
}

impl FakeG502x {
    pub fn new() -> Self {
        let json: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture is JSON");
        let hex_at = |pointer: &str| {
            hex(json
                .pointer(pointer)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("fixture has no string at {pointer}")))
        };
        let fixture = Fixture {
            features: json["features"]
                .as_array()
                .expect("feature list")
                .iter()
                .map(|f| Feature {
                    index: u8::try_from(f["index"].as_u64().expect("index")).expect("u8"),
                    id: u16::from_str_radix(f["id"].as_str().expect("id"), 16).expect("hex id"),
                    version: u8::try_from(f["version"].as_u64().expect("version")).expect("u8"),
                })
                .collect(),
            device_info: hex_at("/device_info"),
            fw_entities: json["fw_entities"]
                .as_array()
                .expect("firmware entities")
                .iter()
                .map(|e| hex(e.as_str().expect("hex")))
                .collect(),
            dpi_list: hex_at("/dpi_sensor0/dpi_list_raw"),
            dpi_current: hex_at("/dpi_sensor0/current_raw"),
            rate_list: hex_at("/report_rate/list_bitmap_raw"),
            rate_current: hex_at("/report_rate/current_raw"),
            description: hex_at("/onboard/description"),
            sectors: json["onboard"]["sectors"]
                .as_object()
                .expect("sectors")
                .iter()
                .map(|(id, data)| {
                    (
                        u16::from_str_radix(id, 16).expect("sector id"),
                        hex(data.as_str().expect("hex")),
                    )
                })
                .collect(),
        };
        let state = State {
            mode: hex_at("/onboard/mode")[0],
            current_profile: hex_at("/onboard/current_profile")[1],
            ignore_profile_switch: false,
            requests: Vec::new(),
        };
        let (responses, reports) = mpsc::unbounded_channel();
        Self {
            fixture,
            state: Arc::new(Mutex::new(state)),
            responses,
            reports: AsyncMutex::new(reports),
        }
    }

    pub fn state(&self) -> Arc<Mutex<State>> {
        Arc::clone(&self.state)
    }

    /// The feature table index the device assigns to `id`.
    pub fn feature_index(&self, id: u16) -> u8 {
        self.fixture
            .features
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.index)
            .expect("feature present in fixture")
    }

    fn respond(&self, request: &[u8]) -> Option<Vec<u8>> {
        let &[
            _report_id,
            device_index,
            feature_index,
            function_sw,
            ref params @ ..,
        ] = request
        else {
            return None;
        };
        let function = function_sw >> 4;
        let result = if feature_index == 0 {
            self.root(function, params)
        } else {
            match self
                .fixture
                .features
                .iter()
                .find(|f| f.index == feature_index)
            {
                Some(feature) => self.feature(feature.id, function, params),
                None => Err(ERR_INVALID_FEATURE_INDEX),
            }
        };

        let mut report = vec![0; LONG_REPORT_LEN];
        report[0] = LONG_REPORT_ID;
        report[1] = device_index;
        match result {
            Ok(payload) => {
                report[2] = feature_index;
                report[3] = function_sw;
                let len = payload.len().min(LONG_REPORT_LEN - 4);
                report[4..4 + len].copy_from_slice(&payload[..len]);
            }
            Err(code) => {
                report[2] = ERROR_FEATURE_INDEX;
                report[3] = feature_index;
                report[4] = function_sw;
                report[5] = code;
            }
        }
        Some(report)
    }

    fn root(&self, function: u8, params: &[u8]) -> Result<Vec<u8>, u8> {
        match function {
            // getFeature(id) -> index, type, version; index 0 when absent.
            0 => {
                let id = u16::from_be_bytes([params[0], params[1]]);
                Ok(match self.fixture.features.iter().find(|f| f.id == id) {
                    Some(feature) => vec![feature.index, 0, feature.version],
                    None => vec![0, 0, 0],
                })
            }
            // getProtocolVersion: HID++ 4.2, echoing the ping byte.
            1 => Ok(vec![4, 2, params[2]]),
            _ => Err(ERR_INVALID_FUNCTION_ID),
        }
    }

    fn feature(&self, id: u16, function: u8, params: &[u8]) -> Result<Vec<u8>, u8> {
        let fx = &self.fixture;
        let mut state = self.state.lock().expect("fake device state");
        match (id, function) {
            (0x0003, 0) => Ok(fx.device_info.clone()),
            (0x0003, 1) => fx
                .fw_entities
                .get(usize::from(params[0]))
                .cloned()
                .ok_or(ERR_INVALID_ARGUMENT),
            (0x2201, 0) => Ok(vec![1]),
            (0x2201, 1) => Ok(fx.dpi_list.clone()),
            (0x2201, 2) => Ok(fx.dpi_current.clone()),
            (0x8060, 0) => Ok(fx.rate_list.clone()),
            (0x8060, 1) => Ok(fx.rate_current.clone()),
            (0x8100, 0) => Ok(fx.description.clone()),
            (0x8100, 2) => Ok(vec![state.mode]),
            (0x8100, 3) => {
                let index = params[1];
                if !(1..=fx.description[3]).contains(&index) {
                    return Err(ERR_INVALID_ARGUMENT);
                }
                if !state.ignore_profile_switch {
                    state.current_profile = index;
                }
                Ok(Vec::new())
            }
            (0x8100, 4) => Ok(vec![0, state.current_profile]),
            (0x8100, 5) => {
                let sector = u16::from_be_bytes([params[0], params[1]]);
                let offset = usize::from(u16::from_be_bytes([params[2], params[3]]));
                let data = fx.sectors.get(&sector).ok_or(ERR_INVALID_ARGUMENT)?;
                data.get(offset..offset + MEMORY_READ_LEN)
                    .map(<[u8]>::to_vec)
                    .ok_or(ERR_INVALID_ARGUMENT)
            }
            _ => Err(ERR_INVALID_FUNCTION_ID),
        }
    }
}

#[async_trait]
impl RawHidChannel for FakeG502x {
    fn vendor_id(&self) -> u16 {
        0x046D
    }

    fn product_id(&self) -> u16 {
        0xC099
    }

    async fn write_report(&self, src: &[u8]) -> Result<usize, BoxError> {
        self.state
            .lock()
            .expect("fake device state")
            .requests
            .push(src.to_vec());
        if let Some(response) = self.respond(src) {
            self.responses.send(response)?;
        }
        Ok(src.len())
    }

    async fn read_report(&self, buf: &mut [u8]) -> Result<usize, BoxError> {
        let Some(report) = self.reports.lock().await.recv().await else {
            // The sender lives as long as the device; never reached while it exists.
            return std::future::pending().await;
        };
        let len = report.len().min(buf.len());
        buf[..len].copy_from_slice(&report[..len]);
        Ok(len)
    }

    fn supports_short_long_hidpp(&self) -> Option<(bool, bool)> {
        Some((true, true))
    }

    async fn get_report_descriptor(&self, _buf: &mut [u8]) -> Result<usize, BoxError> {
        Err("the emulated device has no report descriptor".into())
    }
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("fixture hex"))
        .collect()
}
