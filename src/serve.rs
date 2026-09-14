//! `omalogi serve`: the overlay's connection to the mouse.
//!
//! One JSON request per line on stdin, one JSON response per line on stdout, all over
//! a single device session, so the overlay never waits for a process to start or the
//! whole profile memory to be read again.
//!
//! ```text
//! → {"id": 1, "cmd": "state"}
//! → {"id": 2, "cmd": "apply", "profile": 1, "dpi": [800, 1600], "default_dpi": 1600,
//!    "buttons": {"3": "key:ctrl+t"}}
//! → {"id": 3, "cmd": "undo"}
//! → {"id": 4, "cmd": "activate", "profile": 2}
//! ← {"id": 2, "ok": true, "result": {"slot": {…}, "takes_effect": {"state": "now"}, …}}
//! ← {"id": 3, "ok": false, "error": "…"}
//! ```
//!
//! The device lock is held for each request, never while idle, so the daemon and CLI
//! commands take turns with the server. Writes keep the CLI's guarantees: all profile
//! memory is backed up before the session's first write, every write is read back and
//! verified, and the active profile is loaded again when it changes. The bytes each write
//! replaced are kept, so `undo` puts them back the same way.

use std::{collections::BTreeMap, error::Error, io, path::PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    device::Session,
    editing::{EditError, ProfileChanges, TakesEffect, save_backup},
    error_chain,
    lock::DeviceLock,
    onboard::action::parse_action,
};

/// The HID++ software id of `omalogi serve`, apart from the CLI's and the daemon's, so
/// a CLI command run while the overlay is open never takes the server's replies.
pub const SOFTWARE_ID: u8 = 0x0D;

/// The version of this request format. The plugin, installed and updated on its own with
/// `omarchy plugin add`, asks for a helper update when this is older than it needs.
pub const PROTOCOL: u32 = 1;

/// Where to save a backup for a device name.
pub type BackupPath = fn(&str) -> Result<PathBuf, Box<dyn Error>>;

#[derive(Debug, Deserialize)]
struct Request {
    id: u64,
    #[serde(flatten)]
    command: Command,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    /// Device info and every profile, read from the mouse.
    State,
    Activate {
        profile: usize,
    },
    Apply {
        profile: usize,
        #[serde(flatten)]
        changes: Changes,
    },
    /// Puts back what the latest write in this session replaced.
    Undo,
    /// Turns a profile on or off; answers with every profile.
    SetEnabled {
        profile: usize,
        enabled: bool,
    },
}

/// The changes `profiles edit` takes, as JSON. Slots are keys: `{"3": "key:ctrl+t"}`.
#[derive(Debug, Default, Deserialize)]
struct Changes {
    #[serde(default)]
    dpi: Option<Vec<u16>>,
    #[serde(default)]
    default_dpi: Option<u16>,
    #[serde(default)]
    shift_dpi: Option<u16>,
    #[serde(default)]
    rate: Option<u16>,
    #[serde(default)]
    name: Option<String>,
    // String keys: a flattened struct in a tagged enum cannot read JSON keys as numbers.
    #[serde(default)]
    buttons: BTreeMap<String, String>,
    #[serde(default)]
    gshift: BTreeMap<String, String>,
}

impl Changes {
    fn parse(self) -> Result<ProfileChanges, String> {
        let bindings = |table: BTreeMap<String, String>| {
            table
                .into_iter()
                .map(|(slot, action)| {
                    let number = slot
                        .parse::<usize>()
                        .map_err(|_| format!("`{slot}` is not a slot number"))?;
                    Ok((number, parse_action(&action)?))
                })
                .collect::<Result<Vec<_>, String>>()
        };
        Ok(ProfileChanges {
            dpi_stages: self.dpi,
            default_dpi: self.default_dpi,
            shift_dpi: self.shift_dpi,
            report_rate_hz: self.rate,
            buttons: bindings(self.buttons)?,
            gshift_buttons: bindings(self.gshift)?,
            name: self.name,
        })
    }
}

struct UndoStep {
    profile: usize,
    previous: Vec<u8>,
}

pub struct Server {
    session: Session,
    lock_path: Option<PathBuf>,
    backup_path: BackupPath,
    /// The backup made before this session's first write.
    backup: Option<PathBuf>,
    undo: Vec<UndoStep>,
}

impl Server {
    #[must_use]
    pub fn new(session: Session, lock_path: Option<PathBuf>, backup_path: BackupPath) -> Self {
        Self {
            session,
            lock_path,
            backup_path,
            backup: None,
            undo: Vec::new(),
        }
    }

    /// Answers requests until `input` ends.
    pub async fn run(
        mut self,
        input: impl AsyncBufRead + Unpin,
        mut output: impl AsyncWrite + Unpin,
    ) -> io::Result<()> {
        let mut lines = input.lines();
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<Request>(&line) {
                Ok(request) => {
                    // Each request holds the device lock for all of its traffic: requests
                    // from two processes at once can time out or read the wrong bytes.
                    let outcome = match self.lock() {
                        Ok(_lock) => self.handle(request.command).await,
                        Err(error) => Err(error_chain(error.as_ref())),
                    };
                    match outcome {
                        Ok(result) => json!({ "id": request.id, "ok": true, "result": result }),
                        Err(error) => json!({ "id": request.id, "ok": false, "error": error }),
                    }
                }
                Err(error) => {
                    let id = serde_json::from_str::<Value>(&line)
                        .ok()
                        .and_then(|value| value["id"].as_u64());
                    json!({ "id": id, "ok": false, "error": format!("bad request: {error}") })
                }
            };
            let mut text = response.to_string();
            text.push('\n');
            output.write_all(text.as_bytes()).await?;
            output.flush().await?;
        }
        Ok(())
    }

    async fn handle(&mut self, command: Command) -> Result<Value, String> {
        match command {
            Command::State => {
                let info = self.session.info().await.map_err(|e| error_chain(&e))?;
                let onboard = self.session.onboard().await.map_err(|e| error_chain(&e))?;
                Ok(json!({
                    "info": info,
                    "onboard": onboard,
                    "helper": { "version": env!("CARGO_PKG_VERSION"), "protocol": PROTOCOL },
                }))
            }
            Command::Activate { profile } => {
                self.session
                    .activate_profile(profile)
                    .await
                    .map_err(|e| error_chain(&e))?;
                Ok(json!({ "active_profile": profile }))
            }
            Command::Apply { profile, changes } => {
                let changes = changes.parse()?;
                self.apply(profile, &changes)
                    .await
                    .map_err(|e| error_chain(e.as_ref()))
            }
            Command::Undo => self.undo().await.map_err(|e| error_chain(e.as_ref())),
            Command::SetEnabled { profile, enabled } => self
                .set_enabled(profile, enabled)
                .await
                .map_err(|e| error_chain(e.as_ref())),
        }
    }

    /// Saves a backup of all profile memory before this session's first write.
    async fn ensure_backup(&mut self) -> Result<(), Box<dyn Error>> {
        if self.backup.is_none() {
            let backup = self.session.backup().await?;
            let path = (self.backup_path)(self.session.model().name)?;
            save_backup(&backup, &path)?;
            self.backup = Some(path);
        }
        Ok(())
    }

    async fn set_enabled(
        &mut self,
        profile: usize,
        enabled: bool,
    ) -> Result<Value, Box<dyn Error>> {
        self.session.check_profile_enabled(profile, enabled).await?;
        self.ensure_backup().await?;
        self.session.set_profile_enabled(profile, enabled).await?;
        let onboard = self.session.onboard().await?;
        Ok(json!({ "onboard": onboard, "backup": self.backup }))
    }

    async fn apply(
        &mut self,
        profile: usize,
        changes: &ProfileChanges,
    ) -> Result<Value, Box<dyn Error>> {
        let plan = match self.session.plan_profile_changes(profile, changes).await {
            Ok(plan) => plan,
            // The draft already matches the mouse, e.g. a change was undone by hand.
            Err(EditError::NoChanges) => return self.slot_result(profile, None).await,
            Err(error) => return Err(error.into()),
        };
        self.ensure_backup().await?;
        let takes_effect = self.session.write_plan(&plan).await?;
        self.undo.push(UndoStep {
            profile,
            previous: plan.previous_sector().to_vec(),
        });
        // The write was read back and verified, so the result comes from it, not a new read.
        // Only a profile that was not active reports `when_activated`.
        let active = !matches!(takes_effect, TakesEffect::WhenActivated);
        let slot = self
            .session
            .written_slot(profile, plan.entry(), active, plan.edited_sector())
            .await?;
        Ok(json!({
            "slot": slot,
            "takes_effect": takes_effect,
            "backup": self.backup,
            "undo": self.undo.len(),
        }))
    }

    async fn undo(&mut self) -> Result<Value, Box<dyn Error>> {
        let step = self.undo.pop().ok_or("there is nothing to undo")?;
        let takes_effect = match self
            .session
            .write_profile_sector(step.profile, &step.previous)
            .await
        {
            Ok(takes_effect) => Some(serde_json::to_value(takes_effect)?),
            Err(EditError::NoChanges) => None,
            Err(error) => {
                self.undo.push(step);
                return Err(error.into());
            }
        };
        self.slot_result(step.profile, takes_effect).await
    }

    async fn slot_result(
        &mut self,
        profile: usize,
        takes_effect: Option<Value>,
    ) -> Result<Value, Box<dyn Error>> {
        let slot = self.session.profile_slot(profile).await?;
        Ok(json!({
            "slot": slot,
            "takes_effect": takes_effect,
            "backup": self.backup,
            "undo": self.undo.len(),
        }))
    }

    fn lock(&self) -> Result<Option<DeviceLock>, Box<dyn Error>> {
        let Some(path) = &self.lock_path else {
            return Ok(None);
        };
        let lock = DeviceLock::acquire(path)
            .map_err(|error| format!("could not lock {}: {error}", path.display()))?;
        Ok(Some(lock))
    }
}
