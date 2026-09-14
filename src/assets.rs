//! Pictures of the mouse with the position of each button, for the overlay.
//!
//! Omalogi does not ship Logitech's renders. They are downloaded once, on the user's
//! machine, from the asset host OpenLogi uses, checked against the SHA-256 the host's
//! index lists, and cached under `$XDG_CACHE_HOME/omalogi/pictures`.

use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const ASSET_HOST: &str = "https://assets.openlogi.org/";
const INDEX_NAME: &str = "index.json";
const METADATA_NAME: &str = "metadata.json";
const USER_AGENT: &str = concat!(
    "omalogi/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/elberacasa/omalogi)"
);
/// The index is under 1 MB and the largest render about 6 MB.
const MAX_DOWNLOAD_BYTES: u64 = 32 * 1024 * 1024;

/// Picture views Omalogi shows, by metadata key, with the file each one uses.
const VIEWS: &[(&str, &str, &str)] = &[
    ("device_image", "front", "front.png"),
    ("device_side", "side", "side.png"),
];

/// Depots whose button ids are checked against the device's onboard slots. For these,
/// `<depot>_g<N>_m1` is onboard slot N-1 (docs/hardware-tests.md).
const VERIFIED_DEPOTS: &[&str] = &["g502x"];

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("could not determine the cache directory; set XDG_CACHE_HOME")]
    NoCacheDir,
    #[error("the picture is not downloaded yet; run `omalogi picture` while online")]
    NotCached,
    #[error("the asset host has no picture for product id {product_id:04x}")]
    NoPicture { product_id: u16 },
    #[error("the asset host lists no {name} for {depot}")]
    MissingFile { depot: String, name: String },
    #[error("download of {url} failed")]
    Download {
        url: String,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("{name} does not match the checksum in the asset index")]
    Checksum { name: String },
    #[error("the asset index is not in the expected format")]
    Index(#[source] serde_json::Error),
    #[error("the asset index names an unsafe path: {0}")]
    UnsafePath(String),
    #[error("the button positions for {depot} are not in the expected format")]
    Metadata {
        depot: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not access {path}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
}

/// A picture of the device, ready for the overlay.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Picture {
    pub depot: String,
    /// Whether hotspots are mapped to onboard slots; false for unverified devices.
    pub slots_verified: bool,
    pub views: Vec<View>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct View {
    /// `front` or `side`.
    pub name: &'static str,
    pub image: PathBuf,
    pub width: f64,
    pub height: f64,
    pub hotspots: Vec<Hotspot>,
}

/// A button's position, as a fraction of the picture's width and height.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Hotspot {
    pub slot: usize,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Never download; fail if the picture is not cached.
    pub offline: bool,
    /// Download the index again, and any file that changed upstream.
    pub refresh: bool,
}

#[derive(Deserialize)]
struct Index {
    devices: BTreeMap<String, IndexDevice>,
}

#[derive(Deserialize)]
struct IndexDevice {
    #[serde(rename = "modelIds", default)]
    model_ids: Vec<String>,
    asset_path: String,
    #[serde(default)]
    files: Vec<IndexFile>,
}

#[derive(Deserialize)]
struct IndexFile {
    name: String,
    sha256: String,
    bytes: u64,
}

#[derive(Deserialize)]
struct Metadata {
    images: Vec<MetadataImage>,
}

#[derive(Deserialize)]
struct MetadataImage {
    key: String,
    origin: Size,
    #[serde(default)]
    assignments: Vec<Assignment>,
}

#[derive(Deserialize)]
struct Size {
    width: f64,
    height: f64,
}

#[derive(Deserialize)]
struct Assignment {
    #[serde(rename = "slotId")]
    slot_id: String,
    marker: Point,
}

#[derive(Deserialize)]
struct Point {
    x: f64,
    y: f64,
}

pub fn default_cache_dir() -> Result<PathBuf, AssetError> {
    env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".cache")))
        .map(|dir| dir.join("omalogi/pictures"))
        .ok_or(AssetError::NoCacheDir)
}

/// The picture for a product id, downloading what is missing from [`ASSET_HOST`].
pub fn picture(product_id: u16, options: Options) -> Result<Picture, AssetError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let fetch = |path: &str| {
        let url = format!("{ASSET_HOST}{path}");
        let download = |source| AssetError::Download {
            url: url.clone(),
            source: Box::new(source),
        };
        agent
            .get(&url)
            .call()
            .map_err(download)?
            .body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD_BYTES)
            .read_to_vec()
            .map_err(download)
    };
    load(&default_cache_dir()?, product_id, options, fetch)
}

/// [`picture`] with the cache directory and the downloader supplied.
pub fn load(
    cache: &Path,
    product_id: u16,
    options: Options,
    mut fetch: impl FnMut(&str) -> Result<Vec<u8>, AssetError>,
) -> Result<Picture, AssetError> {
    let index_path = cache.join(INDEX_NAME);
    let index_bytes = match read_optional(&index_path)? {
        Some(bytes) if !options.refresh => bytes,
        cached => {
            if options.offline {
                cached.ok_or(AssetError::NotCached)?
            } else {
                let bytes = fetch(INDEX_NAME)?;
                // Parse before caching, so a bad download never replaces a good index.
                serde_json::from_slice::<Index>(&bytes).map_err(AssetError::Index)?;
                write_atomically(&index_path, &bytes)?;
                bytes
            }
        }
    };
    let index: Index = serde_json::from_slice(&index_bytes).map_err(AssetError::Index)?;
    let model = format!("{product_id:04x}");
    let (depot, device) = index
        .devices
        .iter()
        .find(|(_, device)| {
            device
                .model_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case(&model))
        })
        .ok_or(AssetError::NoPicture { product_id })?;
    check_depot(depot, &device.asset_path)?;

    let mut file = |name: &str| -> Result<PathBuf, AssetError> {
        let listed = device
            .files
            .iter()
            .find(|file| file.name == name)
            .ok_or_else(|| AssetError::MissingFile {
                depot: depot.clone(),
                name: name.to_owned(),
            })?;
        let path = cache.join(depot).join(name);
        if read_optional(&path)?.is_some_and(|bytes| matches(&bytes, listed)) {
            return Ok(path);
        }
        if options.offline {
            return Err(AssetError::NotCached);
        }
        let bytes = fetch(&format!("{}{name}", device.asset_path))?;
        if !matches(&bytes, listed) {
            return Err(AssetError::Checksum {
                name: name.to_owned(),
            });
        }
        write_atomically(&path, &bytes)?;
        Ok(path)
    };

    let metadata_path = file(METADATA_NAME)?;
    let metadata = fs::read(&metadata_path).map_err(|source| io_error(&metadata_path, source))?;
    let metadata: Metadata =
        serde_json::from_slice(&metadata).map_err(|source| AssetError::Metadata {
            depot: depot.clone(),
            source,
        })?;
    let slots_verified = VERIFIED_DEPOTS.contains(&depot.as_str());
    let mut views = Vec::new();
    for image in &metadata.images {
        let Some(&(_, name, file_name)) = VIEWS.iter().find(|(key, ..)| *key == image.key) else {
            continue;
        };
        if image.origin.width <= 0.0 || image.origin.height <= 0.0 {
            continue;
        }
        let image_path = file(file_name)?;
        let hotspots = if slots_verified {
            image
                .assignments
                .iter()
                .filter_map(|assignment| {
                    Some(Hotspot {
                        slot: onboard_slot(depot, &assignment.slot_id)?,
                        x: assignment.marker.x / image.origin.width,
                        y: assignment.marker.y / image.origin.height,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        views.push(View {
            name,
            image: image_path,
            width: image.origin.width,
            height: image.origin.height,
            hotspots,
        });
    }
    Ok(Picture {
        depot: depot.clone(),
        slots_verified,
        views,
    })
}

/// `<depot>_g<N>_m1` is onboard slot N-1 on verified depots; wheel ids have no slot.
fn onboard_slot(depot: &str, slot_id: &str) -> Option<usize> {
    let number = slot_id
        .strip_prefix(depot)?
        .strip_prefix("_g")?
        .strip_suffix("_m1")?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse::<usize>().ok()?.checked_sub(1)
}

/// The depot becomes a directory name and the asset path part of a URL.
fn check_depot(depot: &str, asset_path: &str) -> Result<(), AssetError> {
    let plain = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    };
    let path_ok = asset_path
        .strip_suffix('/')
        .is_some_and(|path| path.split('/').all(plain));
    if plain(depot) && path_ok {
        Ok(())
    } else {
        Err(AssetError::UnsafePath(format!("{depot} {asset_path}")))
    }
}

fn matches(bytes: &[u8], listed: &IndexFile) -> bool {
    bytes.len() as u64 == listed.bytes && sha256_hex(bytes).eq_ignore_ascii_case(&listed.sha256)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            hex.push_str(&format!("{byte:02x}"));
            hex
        })
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, AssetError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_error(path, source)),
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = parent.join(format!(".{name}.tmp"));
    fs::create_dir_all(parent)
        .and_then(|()| fs::write(&temporary, bytes))
        .and_then(|()| fs::rename(&temporary, path))
        .map_err(|source| io_error(path, source))
}

fn io_error(path: &Path, source: io::Error) -> AssetError {
    AssetError::Io {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use serde_json::json;

    use super::*;

    const FRONT: &[u8] = b"front picture";
    const SIDE: &[u8] = b"side picture";

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                env::temp_dir().join(format!("omalogi-assets-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn listed(name: &str, bytes: &[u8]) -> serde_json::Value {
        json!({ "name": name, "sha256": sha256_hex(bytes), "bytes": bytes.len() })
    }

    /// A made-up host: the same formats as the real one, none of its content.
    fn host(depot: &str, front: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let metadata = serde_json::to_vec(&json!({
            "images": [
                { "key": "device_image", "origin": { "width": 200.0, "height": 400.0 },
                  "assignments": [
                    { "slotId": format!("{depot}_g1_m1"), "marker": { "x": 50.0, "y": 100.0 }, "label": { "x": -10, "y": 0 } },
                    { "slotId": format!("{depot}_scroll1_m1"), "marker": { "x": 100.0, "y": 80.0 } }
                  ] },
                { "key": "device_side", "origin": { "width": 100.0, "height": 400.0 },
                  "assignments": [ { "slotId": format!("{depot}_g4_m1"), "marker": { "x": 25.0, "y": 300.0 } } ] },
                { "key": "splash", "origin": { "width": 1.0, "height": 1.0 } }
            ]
        }))
        .unwrap();
        let index = serde_json::to_vec(&json!({
            "generated_from": "test",
            "devices": {
                "other": { "modelIds": ["c08b"], "asset_path": "v1/devices/other/", "files": [] },
                depot: {
                    "modelId": "c099", "modelIds": ["c099"], "asset_path": format!("v1/devices/{depot}/"),
                    "files": [listed("metadata.json", &metadata), listed("front.png", FRONT), listed("side.png", SIDE)]
                }
            }
        }))
        .unwrap();
        BTreeMap::from([
            ("index.json".to_owned(), index),
            (format!("v1/devices/{depot}/metadata.json"), metadata),
            (format!("v1/devices/{depot}/front.png"), front.to_vec()),
            (format!("v1/devices/{depot}/side.png"), SIDE.to_vec()),
        ])
    }

    fn serve<'a>(
        files: &'a BTreeMap<String, Vec<u8>>,
        requests: &'a RefCell<Vec<String>>,
    ) -> impl FnMut(&str) -> Result<Vec<u8>, AssetError> + 'a {
        move |path| {
            requests.borrow_mut().push(path.to_owned());
            files.get(path).cloned().ok_or(AssetError::NotCached)
        }
    }

    const ONLINE: Options = Options {
        offline: false,
        refresh: false,
    };
    const OFFLINE: Options = Options {
        offline: true,
        refresh: false,
    };

    #[test]
    fn downloads_once_then_works_offline() {
        let dir = TempDir::new("cache");
        let files = host("g502x", FRONT);
        let requests = RefCell::new(Vec::new());
        let picture = load(&dir.0, 0xC099, ONLINE, serve(&files, &requests)).unwrap();
        assert_eq!(requests.borrow().len(), 4);
        assert!(picture.slots_verified);
        assert_eq!(picture.views.len(), 2);
        let front = &picture.views[0];
        assert_eq!(front.name, "front");
        assert_eq!(fs::read(&front.image).unwrap(), FRONT);
        // The wheel id has no slot; g1 is slot 0 at a quarter of both dimensions.
        assert_eq!(
            front.hotspots,
            vec![Hotspot {
                slot: 0,
                x: 0.25,
                y: 0.25
            }]
        );
        assert_eq!(picture.views[1].hotspots[0].slot, 3);

        requests.borrow_mut().clear();
        let again = load(&dir.0, 0xC099, OFFLINE, serve(&files, &requests)).unwrap();
        assert_eq!(again, picture);
        assert!(requests.borrow().is_empty());
        load(&dir.0, 0xC099, ONLINE, serve(&files, &requests)).unwrap();
        assert!(
            requests.borrow().is_empty(),
            "a complete cache needs no downloads"
        );
    }

    #[test]
    fn offline_without_cache_fails() {
        let dir = TempDir::new("offline");
        let files = host("g502x", FRONT);
        let requests = RefCell::new(Vec::new());
        let error = load(&dir.0, 0xC099, OFFLINE, serve(&files, &requests)).unwrap_err();
        assert!(matches!(error, AssetError::NotCached));
        assert!(requests.borrow().is_empty());
    }

    #[test]
    fn corrupt_download_is_rejected_and_not_cached() {
        let dir = TempDir::new("corrupt");
        let mut files = host("g502x", FRONT);
        files.insert(
            "v1/devices/g502x/front.png".to_owned(),
            b"tampered".to_vec(),
        );
        let requests = RefCell::new(Vec::new());
        let error = load(&dir.0, 0xC099, ONLINE, serve(&files, &requests)).unwrap_err();
        assert!(matches!(error, AssetError::Checksum { ref name } if name == "front.png"));
        assert!(!dir.0.join("g502x/front.png").exists());
    }

    #[test]
    fn a_changed_cached_file_is_downloaded_again() {
        let dir = TempDir::new("stale");
        let files = host("g502x", FRONT);
        let requests = RefCell::new(Vec::new());
        load(&dir.0, 0xC099, ONLINE, serve(&files, &requests)).unwrap();
        fs::write(dir.0.join("g502x/side.png"), b"damaged").unwrap();
        requests.borrow_mut().clear();
        load(&dir.0, 0xC099, ONLINE, serve(&files, &requests)).unwrap();
        assert_eq!(*requests.borrow(), vec!["v1/devices/g502x/side.png"]);
    }

    #[test]
    fn unverified_devices_get_pictures_without_slots() {
        let dir = TempDir::new("unverified");
        let files = host("g502x_plus", FRONT);
        let requests = RefCell::new(Vec::new());
        let picture = load(&dir.0, 0xC099, ONLINE, serve(&files, &requests)).unwrap();
        assert!(!picture.slots_verified);
        assert!(picture.views.iter().all(|view| view.hotspots.is_empty()));
    }

    #[test]
    fn unknown_products_and_unsafe_paths_are_refused() {
        let dir = TempDir::new("refused");
        let files = host("g502x", FRONT);
        let requests = RefCell::new(Vec::new());
        let error = load(&dir.0, 0x1234, ONLINE, serve(&files, &requests)).unwrap_err();
        assert!(matches!(
            error,
            AssetError::NoPicture { product_id: 0x1234 }
        ));
        assert!(check_depot("g502x", "v1/devices/g502x/").is_ok());
        assert!(check_depot("../x", "v1/devices/x/").is_err());
        assert!(check_depot("g502x", "v1/../../etc/").is_err());
        assert!(check_depot("g502x", "https://elsewhere/").is_err());
    }

    #[test]
    fn slot_ids_map_to_onboard_slots() {
        assert_eq!(onboard_slot("g502x", "g502x_g1_m1"), Some(0));
        assert_eq!(onboard_slot("g502x", "g502x_g11_m1"), Some(10));
        assert_eq!(onboard_slot("g502x", "g502x_scroll1_m1"), None);
        assert_eq!(onboard_slot("g502x", "g502x_g0_m1"), None);
        assert_eq!(onboard_slot("g502x", "g502x_g1_m2"), None);
        assert_eq!(onboard_slot("g502x", "other_g1_m1"), None);
    }
}
