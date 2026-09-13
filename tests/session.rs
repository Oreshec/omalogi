//! End-to-end tests of `Session` against an emulated G502 X.

mod support;

use std::sync::{Arc, Mutex};

use omalogi::{
    device::{Session, SessionError},
    hidraw::SUPPORTED_DEVICES,
    onboard::{
        Mode,
        format::{Binding, SpecialAction},
    },
};
use support::{FakeG502x, State};

const ONBOARD_PROFILES: u16 = 0x8100;
const SET_CURRENT_PROFILE: u8 = 3;

async fn connect() -> (Session, Arc<Mutex<State>>, u8) {
    let device = FakeG502x::new();
    let state = device.state();
    let onboard_index = device.feature_index(ONBOARD_PROFILES);
    let session = Session::connect(device, SUPPORTED_DEVICES[0], "emulated".to_owned())
        .await
        .expect("session starts");
    (session, state, onboard_index)
}

fn profile_switch_requests(state: &Mutex<State>, onboard_index: u8) -> usize {
    state
        .lock()
        .expect("state")
        .requests
        .iter()
        .filter(|r| r[2] == onboard_index && r[3] >> 4 == SET_CURRENT_PROFILE)
        .count()
}

#[tokio::test]
async fn reads_device_info() {
    let (mut session, _, _) = connect().await;
    let info = session.info().await.expect("info");

    let firmware: Vec<_> = info.firmware.iter().map(|f| f.version.as_str()).collect();
    assert_eq!(firmware, ["BL1 59.00.B0002", "U1 60.00.B0009"]);
    assert_eq!(info.dpi, 1600);
    assert_eq!(
        (info.dpi_values.first(), info.dpi_values.last()),
        (Some(&100), Some(&25600))
    );
    assert_eq!(info.report_rate_hz, Some(1000));
    assert_eq!(info.report_rates_hz, [125, 250, 500, 1000]);
    assert_eq!(info.onboard_mode, Mode::Onboard);
}

#[tokio::test]
async fn reads_onboard_profiles() {
    let (mut session, _, _) = connect().await;
    let state = session.onboard().await.expect("onboard state");

    assert_eq!(state.profiles.len(), 5);
    assert_eq!(state.active_position, Some(0));
    assert!(state.profiles.iter().all(|slot| slot.crc_valid));
    let enabled: Vec<_> = state.profiles.iter().map(|slot| slot.enabled).collect();
    assert_eq!(enabled, [true, true, false, false, false]);
    assert_eq!(
        state.profiles[1].profile.buttons[4],
        Binding::Special {
            code: 0x0B,
            action: Some(SpecialAction::GShift),
            profile: 0
        }
    );
}

#[tokio::test]
async fn backup_contains_every_listed_sector() {
    let (mut session, _, _) = connect().await;
    let backup = session.backup().await.expect("backup");

    let sectors: Vec<_> = backup.sectors.keys().map(String::as_str).collect();
    assert_eq!(
        sectors,
        [
            "0000", "0001", "0002", "0003", "0004", "0005", "0100", "0101", "0102"
        ]
    );
    assert!(backup.sectors.values().all(|hex| hex.len() == 255 * 2));
}

#[tokio::test]
async fn activates_an_enabled_profile() {
    let (mut session, state, _) = connect().await;

    session
        .activate_profile(2)
        .await
        .expect("profile 2 activates");

    assert_eq!(state.lock().expect("state").current_profile, 2);
    let onboard = session.onboard().await.expect("onboard state");
    assert_eq!(onboard.active_position, Some(1));
}

#[tokio::test]
async fn refuses_disabled_and_missing_profiles_without_writing() {
    let (mut session, state, onboard_index) = connect().await;

    assert!(matches!(
        session.activate_profile(3).await,
        Err(SessionError::ProfileDisabled(3))
    ));
    for number in [0, 6] {
        assert!(matches!(
            session.activate_profile(number).await,
            Err(SessionError::NoSuchProfile { count: 5, .. })
        ));
    }

    assert_eq!(profile_switch_requests(&state, onboard_index), 0);
    assert_eq!(state.lock().expect("state").current_profile, 1);
}

#[tokio::test]
async fn refuses_to_switch_in_host_mode() {
    let (mut session, state, onboard_index) = connect().await;
    state.lock().expect("state").mode = 2;

    assert!(matches!(
        session.activate_profile(2).await,
        Err(SessionError::NotOnboardMode(Mode::Host))
    ));
    assert_eq!(profile_switch_requests(&state, onboard_index), 0);
}

#[tokio::test]
async fn reports_a_switch_the_device_did_not_apply() {
    let (mut session, state, _) = connect().await;
    state.lock().expect("state").ignore_profile_switch = true;

    assert!(matches!(
        session.activate_profile(2).await,
        Err(SessionError::SwitchNotApplied {
            requested: 2,
            reported: Some(1)
        })
    ));
}
