//! Linux hidraw transport for HID++, and discovery of supported devices.

use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use hidpp::{async_trait, channel::RawHidChannel};
use thiserror::Error;
use tokio::io::unix::AsyncFd;

const SYSFS_HIDRAW: &str = "/sys/class/hidraw";
const HIDPP_USAGE_PAGE: u32 = 0xFF00;
const SHORT_REPORT_ID: u32 = 0x10;
const LONG_REPORT_ID: u32 = 0x11;

/// A Logitech mouse Omalogi knows, wired over USB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: &'static str,
    /// Tested on real hardware; other models are edited only after the user accepts.
    pub verified: bool,
}

const fn verified(product_id: u16, name: &'static str) -> SupportedDevice {
    SupportedDevice {
        vendor_id: 0x046D,
        product_id,
        name,
        verified: true,
    }
}

const fn untested(product_id: u16, name: &'static str) -> SupportedDevice {
    SupportedDevice {
        verified: false,
        ..verified(product_id, name)
    }
}

/// Wired G-series mice with HID++ 2.0 onboard profiles. The verified G502 X comes first;
/// the untested models and their USB ids come from libratbag's device database
/// (`data/devices/*.device`, MIT).
pub const SUPPORTED_DEVICES: &[SupportedDevice] = &[
    verified(0xC099, "G502 X"),
    untested(0xC07D, "G502 Proteus Core"),
    untested(0xC07E, "G402"),
    untested(0xC07F, "G302"),
    untested(0xC080, "G303"),
    untested(0xC081, "G900"),
    untested(0xC082, "G403 Wireless"),
    untested(0xC083, "G403"),
    untested(0xC084, "G102/G203"),
    untested(0xC085, "G Pro"),
    untested(0xC086, "G903"),
    untested(0xC087, "G703"),
    untested(0xC088, "G Pro Wireless"),
    untested(0xC08B, "G502 Hero"),
    untested(0xC08C, "G Pro"),
    untested(0xC08D, "G502 Hero Wireless"),
    untested(0xC08E, "MX518"),
    untested(0xC08F, "G403 Hero"),
    untested(0xC090, "G703 Hero"),
    untested(0xC091, "G903 Hero"),
    untested(0xC092, "G102/G203"),
    untested(0xC094, "G Pro X Superlight"),
    untested(0xC095, "G502 X Plus"),
    untested(0xC096, "G705"),
    untested(0xC097, "G303 Shroud Edition"),
    untested(0xC098, "G502 X Lightspeed"),
    untested(0xC09D, "G102/G203"),
    untested(0xC332, "G502 Proteus Spectrum"),
];

#[derive(Debug, Error)]
pub enum HidrawError {
    #[error(
        "no supported Logitech mouse found; plug a G-series mouse in over USB \
         (wireless receivers are not supported yet)"
    )]
    NotFound,
    #[error(
        "permission denied opening {path}; install Omalogi's udev rule \
         (packaging/udev/70-omalogi.rules) and replug the mouse, \
         or for this session run: sudo setfacl -m u:$USER:rw {path}"
    )]
    PermissionDenied { path: String },
    #[error("could not access {path}")]
    Io { path: String, source: io::Error },
}

/// The HID++ interface of a connected supported device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceNode {
    pub device: SupportedDevice,
    /// `/dev/hidrawN`.
    pub path: PathBuf,
    /// `/sys/class/hidraw/hidrawN`.
    pub sysfs: PathBuf,
}

/// Finds the HID++ interface of the first connected supported device.
pub fn find_supported() -> Result<DeviceNode, HidrawError> {
    find_supported_in(Path::new(SYSFS_HIDRAW))
}

fn find_supported_in(class_dir: &Path) -> Result<DeviceNode, HidrawError> {
    let entries = fs::read_dir(class_dir).map_err(|source| HidrawError::Io {
        path: class_dir.display().to_string(),
        source,
    })?;
    let mut nodes: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    nodes.sort();

    nodes
        .into_iter()
        .find_map(|sysfs| {
            let uevent = fs::read_to_string(sysfs.join("device/uevent")).ok()?;
            let (vendor_id, product_id) = parse_hid_id(&uevent)?;
            let device = *SUPPORTED_DEVICES
                .iter()
                .find(|d| (d.vendor_id, d.product_id) == (vendor_id, product_id))?;
            let descriptor = fs::read(sysfs.join("device/report_descriptor")).ok()?;
            if hidpp_reports(&descriptor) != (true, true) {
                return None;
            }
            let path = Path::new("/dev").join(sysfs.file_name()?);
            Some(DeviceNode {
                device,
                path,
                sysfs,
            })
        })
        .ok_or(HidrawError::NotFound)
}

/// Parses `HID_ID=0003:0000046D:0000C099` from a hidraw parent's `uevent`.
fn parse_hid_id(uevent: &str) -> Option<(u16, u16)> {
    let id = uevent
        .lines()
        .find_map(|line| line.strip_prefix("HID_ID="))?;
    let mut fields = id.split(':').skip(1);
    let vendor = u32::from_str_radix(fields.next()?, 16).ok()?;
    let product = u32::from_str_radix(fields.next()?, 16).ok()?;
    Some((u16::try_from(vendor).ok()?, u16::try_from(product).ok()?))
}

/// Whether a report descriptor declares HID++ short (0x10) and long (0x11)
/// reports under the vendor usage page 0xFF00.
///
/// Walks the descriptor's short items tracking the global Usage Page. Push/Pop
/// are not tracked; HID++ descriptors do not use them.
fn hidpp_reports(descriptor: &[u8]) -> (bool, bool) {
    const LONG_ITEM: u8 = 0xFE;
    const USAGE_PAGE: u8 = 0x04;
    const REPORT_ID: u8 = 0x84;

    let (mut short, mut long) = (false, false);
    let mut usage_page = 0;
    let mut i = 0;
    while let Some(&prefix) = descriptor.get(i) {
        if prefix == LONG_ITEM {
            let Some(&size) = descriptor.get(i + 1) else {
                break;
            };
            i += 3 + usize::from(size);
            continue;
        }
        let size = match prefix & 0x03 {
            3 => 4,
            n => usize::from(n),
        };
        let Some(data) = descriptor.get(i + 1..i + 1 + size) else {
            break;
        };
        let value = data
            .iter()
            .rev()
            .fold(0u32, |acc, &byte| (acc << 8) | u32::from(byte));
        match prefix & 0xFC {
            USAGE_PAGE => usage_page = value,
            REPORT_ID if usage_page == HIDPP_USAGE_PAGE => match value {
                SHORT_REPORT_ID => short = true,
                LONG_REPORT_ID => long = true,
                _ => {}
            },
            _ => {}
        }
        i += 1 + size;
    }
    (short, long)
}

/// Non-blocking hidraw I/O on the Tokio reactor.
pub struct HidrawChannel {
    file: AsyncFd<File>,
    node: DeviceNode,
    connected: AtomicBool,
}

impl HidrawChannel {
    /// Opens the node for reading and writing. Must be called inside a Tokio runtime.
    pub fn open(node: DeviceNode) -> Result<Self, HidrawError> {
        let path = node.path.display().to_string();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&node.path)
            .map_err(|source| match source.kind() {
                io::ErrorKind::PermissionDenied => {
                    HidrawError::PermissionDenied { path: path.clone() }
                }
                _ => HidrawError::Io {
                    path: path.clone(),
                    source,
                },
            })?;
        let file = AsyncFd::new(file).map_err(|source| HidrawError::Io { path, source })?;
        Ok(Self {
            file,
            node,
            connected: AtomicBool::new(true),
        })
    }

    /// Errors that will not clear: the device is gone.
    fn is_permanent(error: &io::Error) -> bool {
        matches!(error.raw_os_error(), Some(libc::ENODEV | libc::EIO))
    }

    fn note(&self, error: io::Error) -> io::Error {
        if Self::is_permanent(&error) {
            self.connected.store(false, Ordering::Relaxed);
        }
        error
    }
}

#[async_trait]
impl RawHidChannel for HidrawChannel {
    fn vendor_id(&self) -> u16 {
        self.node.device.vendor_id
    }

    fn product_id(&self) -> u16 {
        self.node.device.product_id
    }

    async fn write_report(&self, src: &[u8]) -> Result<usize, Box<dyn Error + Sync + Send>> {
        loop {
            let mut guard = self.file.writable().await?;
            if let Ok(result) = guard.try_io(|fd| {
                let mut file: &File = fd.get_ref();
                file.write(src)
            }) {
                return result.map_err(|error| self.note(error).into());
            }
        }
    }

    async fn read_report(&self, buf: &mut [u8]) -> Result<usize, Box<dyn Error + Sync + Send>> {
        loop {
            let mut guard = self.file.readable().await?;
            match guard.try_io(|fd| {
                let mut file: &File = fd.get_ref();
                file.read(buf)
            }) {
                Ok(Ok(len)) => return Ok(len),
                Ok(Err(error)) if Self::is_permanent(&error) => {
                    self.note(error);
                    // The trait contract: park on a permanent failure. The channel's
                    // read loop races this future against its close signal.
                    std::future::pending::<()>().await;
                }
                Ok(Err(error)) => return Err(error.into()),
                Err(_would_block) => {}
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn supports_short_long_hidpp(&self) -> Option<(bool, bool)> {
        // Discovery only returns nodes that declare both report kinds.
        Some((true, true))
    }

    async fn get_report_descriptor(
        &self,
        buf: &mut [u8],
    ) -> Result<usize, Box<dyn Error + Sync + Send>> {
        let descriptor = fs::read(self.node.sysfs.join("device/report_descriptor"))?;
        let len = descriptor.len().min(buf.len());
        buf[..len].copy_from_slice(&descriptor[..len]);
        Ok(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// USB interface 1 of a wired G502 X (046d:c099): keyboard, consumer, and HID++.
    const G502X_IFACE1: &str = "05010906a1018501050719e029e715002501750195088102810395067508150026ff0019002aff008100c0050c0901a1018503751095021501268c0219012a8c028100c005010980a10185047502950115012503098209810983816075068103c00600ff0901a101851075089506150026ff000901810009019100c00600ff0902a101851175089513150026ff000902810009029100c0";
    /// USB interface 0 of the same device: pointer, plus a vendor collection without HID++.
    const G502X_IFACE0: &str = "05010902a1010901a10005091901291015002501951075018102050116018026ff7f751095020930093181061581257f7508950109388106050c0a380295018106c00600ff09f175089505150026ff008100c0";

    fn bytes(hex: &str) -> Vec<u8> {
        let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("descriptor hex"))
            .collect()
    }

    #[test]
    fn detects_hidpp_interface() {
        assert_eq!(hidpp_reports(&bytes(G502X_IFACE1)), (true, true));
    }

    #[test]
    fn ignores_pointer_interface() {
        assert_eq!(hidpp_reports(&bytes(G502X_IFACE0)), (false, false));
    }

    #[test]
    fn report_ids_outside_vendor_page_do_not_count() {
        // Usage Page (Generic Desktop), Report ID 0x10, Report ID 0x11.
        assert_eq!(
            hidpp_reports(&[0x05, 0x01, 0x85, 0x10, 0x85, 0x11]),
            (false, false)
        );
    }

    #[test]
    fn truncated_descriptor_does_not_panic() {
        assert_eq!(hidpp_reports(&[0x06, 0x00]), (false, false));
        assert_eq!(hidpp_reports(&[0xFE]), (false, false));
    }

    #[test]
    fn parses_hid_id_from_uevent() {
        let uevent =
            "DRIVER=hid-generic\nHID_ID=0003:0000046D:0000C099\nHID_NAME=Logitech G502 X\n";
        assert_eq!(parse_hid_id(uevent), Some((0x046D, 0xC099)));
        assert_eq!(parse_hid_id("HID_NAME=nothing\n"), None);
    }

    #[test]
    fn finds_device_in_sysfs_tree() {
        let root = std::env::temp_dir().join(format!("omalogi-sysfs-{}", std::process::id()));
        for (node, uevent, descriptor) in [
            ("hidraw7", "HID_ID=0003:0000046D:0000C099\n", G502X_IFACE0),
            ("hidraw8", "HID_ID=0003:0000046D:0000C099\n", G502X_IFACE1),
            ("hidraw9", "HID_ID=0003:00001B1C:00000C1A\n", G502X_IFACE1),
        ] {
            let dir = root.join(node).join("device");
            fs::create_dir_all(&dir).expect("create fake sysfs");
            fs::write(dir.join("uevent"), uevent).expect("write uevent");
            fs::write(dir.join("report_descriptor"), bytes(descriptor)).expect("write descriptor");
        }

        let found = find_supported_in(&root).expect("device found");
        fs::remove_dir_all(&root).expect("clean up fake sysfs");

        assert_eq!(found.path, Path::new("/dev/hidraw8"));
        assert_eq!(found.device.product_id, 0xC099);
        assert!(found.device.verified);
    }

    #[test]
    fn finds_an_untested_known_mouse_but_not_unknown_devices() {
        let root =
            std::env::temp_dir().join(format!("omalogi-sysfs-untested-{}", std::process::id()));
        for (node, uevent) in [
            // A Lightspeed receiver: not a mouse Omalogi opens directly.
            ("hidraw3", "HID_ID=0003:0000046D:0000C539\n"),
            ("hidraw4", "HID_ID=0003:0000046D:0000C08B\n"),
        ] {
            let dir = root.join(node).join("device");
            fs::create_dir_all(&dir).expect("create fake sysfs");
            fs::write(dir.join("uevent"), uevent).expect("write uevent");
            fs::write(dir.join("report_descriptor"), bytes(G502X_IFACE1))
                .expect("write descriptor");
        }

        let found = find_supported_in(&root).expect("device found");
        fs::remove_dir_all(&root).expect("clean up fake sysfs");

        assert_eq!(found.path, Path::new("/dev/hidraw4"));
        assert_eq!(found.device.name, "G502 Hero");
        assert!(!found.device.verified);
    }

    #[test]
    fn the_verified_mouse_is_listed_first_and_ids_are_unique() {
        assert!(SUPPORTED_DEVICES[0].verified);
        assert_eq!(SUPPORTED_DEVICES[0].product_id, 0xC099);
        assert_eq!(SUPPORTED_DEVICES.iter().filter(|d| d.verified).count(), 1);
        let mut ids: Vec<u16> = SUPPORTED_DEVICES.iter().map(|d| d.product_id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SUPPORTED_DEVICES.len());
    }
}
