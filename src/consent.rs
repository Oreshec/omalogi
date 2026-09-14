//! Consent to edit a mouse Omalogi has not been verified on.
//!
//! Only the wired G502 X, with profile layout (1, 4), has been verified on real hardware.
//! Other Logitech mice whose onboard memory Omalogi can read are edited only after the
//! user accepts, once per model and layout. Every write is still backed up, read back and
//! verified. Acceptances are kept in `$XDG_STATE_HOME/omalogi/untested.json`.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{device::Session, editing::EditError, onboard::format::Description};

/// A mouse model and profile layout, as accepted for editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UntestedModel {
    pub product_id: u16,
    pub memory_model: u8,
    pub profile_format: u8,
}

impl UntestedModel {
    fn new(product_id: u16, description: &Description) -> Self {
        Self {
            product_id,
            memory_model: description.memory_model,
            profile_format: description.profile_format,
        }
    }
}

/// How far Omalogi's support for the connected mouse has been verified.
#[derive(Debug, Clone, Serialize)]
pub struct Support {
    pub name: &'static str,
    /// The model and its profile layout were verified on real hardware.
    pub verified: bool,
    /// Omalogi can read and edit this profile layout.
    pub editable: bool,
    /// Edits are allowed: the mouse is verified, or the user accepted it.
    pub accepted: bool,
    pub memory_model: u8,
    pub profile_format: u8,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    accepted: Vec<UntestedModel>,
}

/// `$XDG_STATE_HOME/omalogi/untested.json`, or `~/.local/state/omalogi/untested.json`.
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".local/state")))
        .map(|dir| dir.join("omalogi/untested.json"))
}

fn read(path: &Path) -> Store {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Whether `model` was accepted for editing.
#[must_use]
pub fn is_accepted(path: &Path, model: UntestedModel) -> bool {
    read(path).accepted.contains(&model)
}

/// Records that `model` may be edited. Written to a temporary file and renamed.
pub fn accept(path: &Path, model: UntestedModel) -> io::Result<()> {
    let mut store = read(path);
    if store.accepted.contains(&model) {
        return Ok(());
    }
    store.accepted.push(model);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(&store).expect("the store always serializes");
    fs::write(&temporary, json)?;
    fs::rename(temporary, path)
}

impl Session {
    /// How well the connected mouse is supported, and whether edits are allowed.
    pub async fn support(&mut self) -> Result<Support, EditError> {
        let description = self.onboard_feature().await?.description().await?;
        let model = self.model();
        let verified = model.verified && description.is_verified();
        let accepted = verified
            || self.consent_path().is_some_and(|path| {
                is_accepted(path, UntestedModel::new(model.product_id, &description))
            });
        Ok(Support {
            name: model.name,
            verified,
            editable: description.is_decodable(),
            accepted,
            memory_model: description.memory_model,
            profile_format: description.profile_format,
        })
    }

    /// Accepts editing this untested mouse from now on. A verified mouse needs nothing.
    pub async fn accept_untested(&mut self) -> Result<Support, EditError> {
        let support = self.support().await?;
        if support.accepted {
            return Ok(support);
        }
        let path = self
            .consent_path()
            .ok_or(EditError::NoStateDirectory)?
            .to_owned();
        let model = UntestedModel {
            product_id: self.model().product_id,
            memory_model: support.memory_model,
            profile_format: support.profile_format,
        };
        accept(&path, model).map_err(|source| EditError::SaveConsent {
            path: path.display().to_string(),
            source,
        })?;
        self.support().await
    }

    /// Refuses writes to an untested mouse the user has not accepted.
    pub(crate) async fn ensure_writes_accepted(&mut self) -> Result<(), EditError> {
        let support = self.support().await?;
        if support.accepted {
            Ok(())
        } else {
            Err(EditError::NotAccepted { name: support.name })
        }
    }
}
