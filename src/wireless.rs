//! Mice behind LIGHTSPEED, Bolt and Unifying receivers.
//!
//! Receivers, their paired devices, and the route to each one come from OpenLogi's device
//! layer (`openlogi-hid`, MIT OR Apache-2.0), which already speaks the receiver protocols.
//! Omalogi picks the mice it knows from that inventory and opens a HID++ channel at the
//! mouse's receiver slot; everything above the channel is the same as for a wired mouse.

use std::sync::Arc;

use hidpp::channel::HidppChannel;
use openlogi_core::device::{DeviceInventory, DeviceKind};
use openlogi_hid::DeviceRoute;
use thiserror::Error;

use crate::hidraw::{SupportedDevice, WIRELESS_DEVICES};

#[derive(Debug, Error)]
#[error("could not look for wireless mice: {0}")]
pub struct WirelessError(String);

/// A known wireless mouse, online behind a receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WirelessMouse {
    pub model: SupportedDevice,
    pub route: DeviceRoute,
    /// Where it was found, e.g. `Lightspeed Receiver, slot 1`.
    pub path: String,
}

/// The online wireless mice Omalogi knows, from an OpenLogi inventory. Mice attached
/// directly are left to the wired path, which finds them faster.
#[must_use]
pub fn known_mice(inventories: &[DeviceInventory]) -> Vec<WirelessMouse> {
    inventories
        .iter()
        .flat_map(|inventory| {
            inventory.paired.iter().filter_map(move |device| {
                if device.kind != DeviceKind::Mouse || !device.online {
                    return None;
                }
                // The WPID when the receiver reported it, else the mouse's own model ids.
                let mut ids = device.wpid.into_iter().chain(
                    device
                        .model_info
                        .iter()
                        .flat_map(|info| info.model_ids.iter().copied()),
                );
                let model = ids.find_map(|id| {
                    WIRELESS_DEVICES
                        .iter()
                        .find(|model| model.product_id == id)
                        .copied()
                })?;
                let route = DeviceRoute::for_slot(inventory, device.slot)?;
                if matches!(route, DeviceRoute::Direct { .. }) {
                    return None;
                }
                Some(WirelessMouse {
                    model,
                    route,
                    path: format!("{}, slot {}", inventory.receiver.name, device.slot),
                })
            })
        })
        .collect()
}

/// The known wireless mice online right now.
pub async fn find() -> Result<Vec<WirelessMouse>, WirelessError> {
    let inventories = openlogi_hid::enumerate()
        .await
        .map_err(|error| WirelessError(error.to_string()))?;
    Ok(known_mice(&inventories))
}

/// The first known wireless mouse, with a HID++ channel to its receiver.
pub async fn open() -> Result<Option<(WirelessMouse, Arc<HidppChannel>)>, WirelessError> {
    let Some(mouse) = find().await?.into_iter().next() else {
        return Ok(None);
    };
    let channel = openlogi_hid::channel_pool()
        .open(&mouse.route)
        .await
        .map_err(|error| WirelessError(error.to_string()))?
        .ok_or_else(|| {
            WirelessError(format!("the receiver of the {} is gone", mouse.model.name))
        })?;
    Ok(Some((mouse, channel)))
}

#[cfg(test)]
mod tests {
    use openlogi_core::device::{PairedDevice, ReceiverInfo};

    use super::*;

    fn paired(slot: u8, wpid: Option<u16>, kind: DeviceKind, online: bool) -> PairedDevice {
        PairedDevice {
            slot,
            codename: None,
            wpid,
            kind,
            online,
            battery: None,
            model_info: None,
            capabilities: None,
        }
    }

    fn receiver(
        product_id: u16,
        unique_id: Option<&str>,
        paired: Vec<PairedDevice>,
    ) -> DeviceInventory {
        DeviceInventory {
            receiver: ReceiverInfo {
                name: "Lightspeed Receiver".to_owned(),
                vendor_id: 0x046D,
                product_id,
                unique_id: unique_id.map(str::to_owned),
            },
            paired,
        }
    }

    #[test]
    fn picks_online_known_mice_behind_receivers() {
        let inventories = [receiver(
            0xC547,
            Some("receiver-1"),
            vec![
                paired(1, Some(0x409F), DeviceKind::Mouse, true),
                // Asleep, a keyboard, and a mouse Omalogi does not know.
                paired(2, Some(0x4093), DeviceKind::Mouse, false),
                paired(3, Some(0x407C), DeviceKind::Keyboard, true),
                paired(4, Some(0xB023), DeviceKind::Mouse, true),
            ],
        )];
        let mice = known_mice(&inventories);
        assert_eq!(mice.len(), 1);
        assert_eq!(mice[0].model.name, "G502 X Lightspeed");
        assert_eq!(mice[0].route.device_index(), 1);
        assert!(matches!(mice[0].route, DeviceRoute::Unifying { .. }));
        assert_eq!(mice[0].path, "Lightspeed Receiver, slot 1");
    }

    #[test]
    fn leaves_direct_devices_and_unknown_receivers_alone() {
        let direct = receiver(
            0xC099,
            None,
            vec![paired(0xFF, Some(0x409F), DeviceKind::Mouse, true)],
        );
        // Without its unique id a receiver cannot be told apart, so nothing is routed to it.
        let anonymous = receiver(
            0xC547,
            None,
            vec![paired(1, Some(0x409F), DeviceKind::Mouse, true)],
        );
        assert!(known_mice(&[direct, anonymous]).is_empty());
    }
}
