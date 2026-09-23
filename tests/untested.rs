//! Mice Omalogi has not been verified on: read freely, edited only after the user accepts.

mod support;

use std::path::PathBuf;

use omalogi::{
    device::{CLI_SOFTWARE_ID, Session},
    editing::{EditError, ProfileChanges},
    hidraw::SUPPORTED_DEVICES,
};
use support::FakeG502x;

const ONBOARD_PROFILES: u16 = 0x8100;
const WRITE_FUNCTIONS: [u8; 3] = [6, 7, 8];

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("omalogi-untested-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

async fn connect(device: FakeG502x, product_id: u16, consent: Option<PathBuf>) -> Session {
    let model = *SUPPORTED_DEVICES
        .iter()
        .find(|device| device.product_id == product_id)
        .expect("a known model");
    let mut session = Session::connect(device, model, "emulated".to_owned(), CLI_SOFTWARE_ID)
        .await
        .expect("session starts");
    session.set_consent_path(consent);
    session
}

fn rate_change() -> ProfileChanges {
    ProfileChanges {
        report_rate_hz: Some(500),
        ..ProfileChanges::default()
    }
}

#[tokio::test]
async fn an_untested_mouse_is_read_but_edited_only_after_accepting() {
    let dir = temp("accept");
    let device = FakeG502x::new().with_product_id(0xC07D);
    let state = device.state();
    let onboard = device.feature_index(ONBOARD_PROFILES);
    let writes = move || {
        state
            .lock()
            .expect("state")
            .requests
            .iter()
            .filter(|r| r[2] == onboard && WRITE_FUNCTIONS.contains(&(r[3] >> 4)))
            .count()
    };
    let mut session = connect(device, 0xC07D, Some(dir.join("untested.json"))).await;

    let support = session.support().await.expect("support");
    assert_eq!(support.name, "G502 Proteus Core");
    assert!(!support.verified && support.editable && !support.accepted);
    assert_eq!(
        session
            .onboard()
            .await
            .expect("profiles read")
            .profiles
            .len(),
        5
    );

    let refused = session
        .apply_profile_changes(2, &rate_change(), &dir.join("backup-1.json"))
        .await;
    assert!(matches!(
        refused,
        Err(EditError::NotAccepted {
            name: "G502 Proteus Core"
        })
    ));
    assert_eq!(writes(), 0, "nothing was written");
    assert!(
        !dir.join("backup-1.json").exists(),
        "and no backup was made"
    );
    assert!(
        session
            .plan_profile_changes(2, &rate_change())
            .await
            .is_ok(),
        "dry runs still work"
    );

    assert!(session.accept_untested().await.expect("accepted").accepted);
    session
        .apply_profile_changes(2, &rate_change(), &dir.join("backup-2.json"))
        .await
        .expect("written after accepting");
    assert!(writes() > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn acceptance_is_remembered_per_model_and_layout() {
    let dir = temp("remember");
    let path = dir.join("untested.json");
    let mut first = connect(
        FakeG502x::new().with_product_id(0xC07D),
        0xC07D,
        Some(path.clone()),
    )
    .await;
    first.accept_untested().await.expect("accepted");

    let mut again = connect(
        FakeG502x::new().with_product_id(0xC07D),
        0xC07D,
        Some(path.clone()),
    )
    .await;
    assert!(again.support().await.expect("support").accepted);

    // Another model, or the same model reporting another layout, asks again.
    let mut other = connect(
        FakeG502x::new().with_product_id(0xC094),
        0xC094,
        Some(path.clone()),
    )
    .await;
    assert!(!other.support().await.expect("support").accepted);
    let mut relaid = connect(
        FakeG502x::new()
            .with_product_id(0xC07D)
            .with_profile_format(5),
        0xC07D,
        Some(path),
    )
    .await;
    assert!(!relaid.support().await.expect("support").accepted);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_verified_mouse_needs_no_acceptance_but_an_untested_layout_does() {
    let dir = temp("verified");
    let mut verified = connect(FakeG502x::new(), 0xC099, None).await;
    let support = verified.support().await.expect("support");
    assert!(support.verified && support.editable && support.accepted);
    verified
        .apply_profile_changes(2, &rate_change(), &dir.join("backup.json"))
        .await
        .expect("written without any acceptance");

    let mut relaid = connect(FakeG502x::new().with_profile_format(5), 0xC099, None).await;
    let support = relaid.support().await.expect("support");
    assert!(!support.verified && support.editable && !support.accepted);
    assert!(matches!(
        relaid.accept_untested().await,
        Err(EditError::NoStateDirectory)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_layout_omalogi_cannot_read_is_not_editable() {
    let mut session = connect(
        FakeG502x::new()
            .with_product_id(0xC07D)
            .with_profile_format(6),
        0xC07D,
        None,
    )
    .await;
    assert!(!session.support().await.expect("support").editable);
    assert!(
        session
            .plan_profile_changes(2, &rate_change())
            .await
            .is_err()
    );
}
