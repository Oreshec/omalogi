//! A mouse behind a receiver: the same session, addressed at its receiver slot.

mod support;

use omalogi::{
    device::{CLI_SOFTWARE_ID, Session},
    hidraw::WIRELESS_DEVICES,
};
use support::FakeG502x;

#[tokio::test]
async fn a_session_reaches_a_mouse_at_its_receiver_slot() {
    let device = FakeG502x::new();
    let state = device.state();
    let model = *WIRELESS_DEVICES
        .iter()
        .find(|model| model.product_id == 0x409F)
        .expect("a known wireless model");
    let mut session = Session::connect_at(
        device,
        model,
        "Lightspeed Receiver, slot 1".to_owned(),
        CLI_SOFTWARE_ID,
        1,
    )
    .await
    .expect("session starts");

    let onboard = session.onboard().await.expect("profiles read");
    assert_eq!(onboard.profiles.len(), 5);
    let support = session.support().await.expect("support");
    assert_eq!(support.name, "G502 X Lightspeed");
    assert!(!support.verified, "wireless models are untested");

    let requests = state.lock().expect("state").requests.clone();
    assert!(!requests.is_empty());
    assert!(
        requests.iter().all(|request| request[1] == 1),
        "every request addressed receiver slot 1"
    );
}
