//! `omalogi daemon`: switches onboard profiles as Hyprland focus changes, and publishes
//! what it did to `$XDG_RUNTIME_DIR/omalogi/state.json` for the shell plugin.
//!
//! Profiles only change when focus changes, so a profile picked on the mouse stays
//! active until the next focus change.

use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use hidpp::channel::ChannelError;
use serde::Serialize;
use thiserror::Error;
use tokio::{
    signal::unix::{SignalKind, signal},
    time::MissedTickBehavior,
};

use crate::{
    device::{DAEMON_SOFTWARE_ID, Session, SessionError},
    error_chain,
    hyprland::{self, Event, HyprlandError},
    lock::DeviceLock,
    rules::{Config, Focus, Reason},
};

/// How often the active profile is read back, to notice changes made on the mouse.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error(transparent)]
    Hyprland(#[from] HyprlandError),
    #[error("Hyprland closed its event socket")]
    HyprlandClosed,
    #[error("XDG_RUNTIME_DIR is not set; the daemon has nowhere to publish its state")]
    NoRuntimeDir,
    #[error("could not write {path}")]
    State {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("could not listen for termination signals")]
    Signal(#[source] io::Error),
}

/// What the shell plugin reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct State {
    pub connected: bool,
    pub app: Option<String>,
    pub monitor: Option<String>,
    pub active_profile: Option<usize>,
    /// What set the active profile: `"rule 2"` or `"default"`; `None` when it was
    /// chosen on the mouse or before the daemon started.
    pub source: Option<String>,
    pub error: Option<String>,
}

/// `$XDG_RUNTIME_DIR/omalogi/state.json`.
pub fn state_path() -> Result<PathBuf, DaemonError> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("omalogi/state.json"))
        .ok_or(DaemonError::NoRuntimeDir)
}

/// The rules file, reloaded when its modification time changes.
struct ConfigWatch {
    path: PathBuf,
    stamp: Option<(SystemTime, u64)>,
    config: Config,
    error: Option<String>,
}

impl ConfigWatch {
    fn new(path: PathBuf) -> Self {
        let mut watch = Self {
            path,
            stamp: None,
            config: Config::default(),
            error: None,
        };
        watch.reload();
        watch
    }

    /// Modification time and length; a change in either means the file changed.
    fn current_stamp(&self) -> Option<(SystemTime, u64)> {
        let metadata = std::fs::metadata(&self.path).ok()?;
        Some((metadata.modified().ok()?, metadata.len()))
    }

    /// Reloads when the file changed since the last load; true when it did.
    fn reload_if_changed(&mut self) -> bool {
        if self.current_stamp() == self.stamp {
            return false;
        }
        self.reload();
        true
    }

    /// A broken edit keeps the last good rules and reports the problem.
    fn reload(&mut self) {
        self.stamp = self.current_stamp();
        match Config::load(&self.path) {
            Ok(config) => {
                self.config = config;
                self.error = None;
            }
            Err(error) => self.error = Some(error_chain(&error)),
        }
    }
}

struct Daemon {
    config: ConfigWatch,
    session: Option<Session>,
    focus: Focus,
    state: State,
    published: Option<State>,
    state_path: PathBuf,
    device_error: Option<String>,
    rule_error: Option<String>,
    lock_path: Option<PathBuf>,
    lock_warned: bool,
    /// Rules to apply once the device is free again.
    rules_pending: bool,
    /// Consecutive requests that stalled; the session is dropped on the second.
    transient_failures: u8,
}

/// Whether the daemon may use the device now.
enum Access {
    /// Holds the device lock, or runs without one when no lock file can be used.
    Granted(Option<DeviceLock>),
    /// Another Omalogi process is writing profile memory.
    Busy,
}

pub async fn run(config_path: PathBuf) -> Result<(), DaemonError> {
    let state_path = state_path()?;
    let mut events = hyprland::EventStream::connect().await?;
    let focus = hyprland::current_focus().await?;
    let mut terminate = signal(SignalKind::terminate()).map_err(DaemonError::Signal)?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(DaemonError::Signal)?;

    eprintln!(
        "omalogi daemon: rules from {}, state in {}",
        config_path.display(),
        state_path.display()
    );
    let mut daemon = Daemon::new(ConfigWatch::new(config_path), state_path, focus);
    daemon.on_tick().await;
    daemon.publish()?;

    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await;

    let result = loop {
        tokio::select! {
            event = events.next() => match event {
                Ok(Some(event)) => {
                    if daemon.focus_changed(event) {
                        daemon.on_focus_changed().await;
                    }
                }
                Ok(None) => break Err(DaemonError::HyprlandClosed),
                Err(error) => break Err(error.into()),
            },
            _ = ticker.tick() => daemon.on_tick().await,
            _ = terminate.recv() => break Ok(()),
            _ = interrupt.recv() => break Ok(()),
        }
        if let Err(error) = daemon.publish() {
            break Err(error);
        }
    };

    // State must not outlive the daemon, or the plugin would show a stale profile.
    if let Err(error) = std::fs::remove_file(&daemon.state_path)
        && error.kind() != io::ErrorKind::NotFound
    {
        eprintln!(
            "omalogi daemon: could not remove {}: {error}",
            daemon.state_path.display()
        );
    }
    result
}

impl Daemon {
    fn new(config: ConfigWatch, state_path: PathBuf, focus: Focus) -> Self {
        let state = State {
            app: focus.app.clone(),
            monitor: focus.monitor.clone(),
            ..State::default()
        };
        Self {
            config,
            session: None,
            focus,
            state,
            published: None,
            state_path,
            device_error: None,
            rule_error: None,
            lock_path: DeviceLock::default_path(),
            lock_warned: false,
            rules_pending: false,
            transient_failures: 0,
        }
    }

    /// Periodic work: reconnect, read the active profile, apply pending or edited rules.
    /// Skipped while another Omalogi process is writing profile memory.
    async fn on_tick(&mut self) {
        let Access::Granted(_lock) = self.device_access() else {
            return;
        };
        self.refresh_device().await;
        if std::mem::take(&mut self.rules_pending) {
            self.apply_rules().await;
        }
    }

    /// Applies rules for the new focus, or on the next tick if the device is busy.
    async fn on_focus_changed(&mut self) {
        match self.device_access() {
            Access::Granted(_lock) => self.apply_rules().await,
            Access::Busy => self.rules_pending = true,
        }
    }

    fn device_access(&mut self) -> Access {
        let Some(path) = &self.lock_path else {
            return Access::Granted(None);
        };
        match DeviceLock::try_acquire(path) {
            Ok(Some(lock)) => Access::Granted(Some(lock)),
            Ok(None) => Access::Busy,
            Err(error) => {
                if !std::mem::replace(&mut self.lock_warned, true) {
                    eprintln!(
                        "omalogi daemon: could not use {}: {error}; continuing without the device lock",
                        path.display()
                    );
                }
                Access::Granted(None)
            }
        }
    }

    /// Records a focus event; true when the focus actually changed.
    fn focus_changed(&mut self, event: Event) -> bool {
        let before = self.focus.clone();
        match event {
            Event::ActiveWindow { app } => self.focus.app = app,
            Event::FocusedMonitor { name } => self.focus.monitor = Some(name),
        }
        self.state.app.clone_from(&self.focus.app);
        self.state.monitor.clone_from(&self.focus.monitor);
        self.focus != before
    }

    async fn apply_rules(&mut self) {
        self.config.reload_if_changed();
        self.rule_error = None;
        let Some((profile, reason)) = self.config.config.profile_for(&self.focus) else {
            return;
        };
        let source = match reason {
            Reason::Rule(index) => format!("rule {}", index + 1),
            Reason::Default => "default".to_owned(),
        };
        if self.state.active_profile == Some(profile) {
            self.state.source = Some(source);
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match session.activate_profile(profile).await {
            Ok(()) => {
                eprintln!(
                    "omalogi daemon: {} on {}: profile {profile} ({source})",
                    self.focus.app.as_deref().unwrap_or("desktop"),
                    self.focus.monitor.as_deref().unwrap_or("unknown monitor"),
                );
                self.state.active_profile = Some(profile);
                self.state.source = Some(source);
                self.transient_failures = 0;
            }
            Err(error) => self.handle_error(error, Some(&source)),
        }
    }

    /// Opens the device if needed, then reads the active profile back.
    async fn refresh_device(&mut self) {
        if self.session.is_none() {
            match Session::open(DAEMON_SOFTWARE_ID).await {
                Ok(session) => {
                    eprintln!("omalogi daemon: device connected");
                    self.session = Some(session);
                    self.device_error = None;
                    self.state.connected = true;
                    self.read_active_profile().await;
                    self.apply_rules().await;
                }
                Err(error) => {
                    self.device_error = Some(error_chain(&error));
                    self.disconnect();
                }
            }
            return;
        }
        self.read_active_profile().await;
        // A saved config applies right away, not only on the next focus change.
        if self.config.reload_if_changed() {
            self.apply_rules().await;
        }
    }

    async fn read_active_profile(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match session.active_profile().await {
            Ok(profile) => {
                self.transient_failures = 0;
                if profile != self.state.active_profile {
                    self.state.active_profile = profile;
                    self.state.source = None;
                }
            }
            Err(error) => self.handle_error(error, None),
        }
    }

    /// Rule problems are reported and the device stays open. A single stalled request
    /// is retried on the next poll; anything else drops the session so the next poll
    /// reconnects.
    fn handle_error(&mut self, error: SessionError, source: Option<&str>) {
        let message = error_chain(&error);
        match error {
            SessionError::NoSuchProfile { .. }
            | SessionError::ProfileDisabled(_)
            | SessionError::NotOnboardMode(_) => {
                self.rule_error = Some(match source {
                    Some(source) => format!("{source}: {message}"),
                    None => message,
                });
            }
            _ if is_transient(&error) && self.transient_failures == 0 => {
                self.transient_failures = 1;
                if source.is_some() {
                    self.rules_pending = true;
                }
                eprintln!("omalogi daemon: device request stalled, retrying next poll: {message}");
            }
            _ => {
                eprintln!("omalogi daemon: device lost: {message}");
                self.transient_failures = 0;
                self.device_error = Some(message);
                self.disconnect();
            }
        }
    }

    fn disconnect(&mut self) {
        self.session = None;
        self.state.connected = false;
        self.state.active_profile = None;
        self.state.source = None;
    }

    fn current_state(&self) -> State {
        State {
            error: self
                .config
                .error
                .clone()
                .or_else(|| self.device_error.clone())
                .or_else(|| self.rule_error.clone()),
            ..self.state.clone()
        }
    }

    fn publish(&mut self) -> Result<(), DaemonError> {
        let state = self.current_state();
        if self.published.as_ref() == Some(&state) {
            return Ok(());
        }
        write_atomically(&self.state_path, &state)?;
        self.published = Some(state);
        Ok(())
    }
}

/// Whether an error is a request that stalled or timed out, rather than a missing device.
fn is_transient(error: &SessionError) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = current {
        if let Some(channel) = error.downcast_ref::<ChannelError>()
            && matches!(channel, ChannelError::Timeout | ChannelError::NoResponse)
        {
            return true;
        }
        if let Some(io) = error.downcast_ref::<io::Error>()
            && io.raw_os_error() == Some(libc::ETIMEDOUT)
        {
            return true;
        }
        current = error.source();
    }
    false
}

fn write_atomically(path: &Path, state: &State) -> Result<(), DaemonError> {
    let error = |source| DaemonError::State {
        path: path.display().to_string(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(error)?;
    }
    let temporary = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(state).expect("state always serializes");
    std::fs::write(&temporary, json).map_err(error)?;
    std::fs::rename(&temporary, path).map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omalogi-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn daemon(dir: &Path) -> Daemon {
        Daemon::new(
            ConfigWatch::new(dir.join("config.toml")),
            dir.join("state.json"),
            Focus::default(),
        )
    }

    #[test]
    fn stalled_requests_are_transient_but_missing_devices_are_not() {
        use crate::hidraw::HidrawError;
        use hidpp::protocol::v20::Hidpp20Error;

        let timeout = SessionError::Request(Hidpp20Error::Channel(ChannelError::Timeout));
        assert!(is_transient(&timeout));

        // The failure observed on hardware: hidraw write returned ETIMEDOUT.
        let stalled_write = SessionError::Request(Hidpp20Error::Channel(
            ChannelError::Implementation(Box::new(io::Error::from_raw_os_error(libc::ETIMEDOUT))),
        ));
        assert!(is_transient(&stalled_write));

        let gone = SessionError::Request(Hidpp20Error::Channel(ChannelError::Implementation(
            Box::new(io::Error::from_raw_os_error(libc::ENODEV)),
        )));
        assert!(!is_transient(&gone));
        assert!(!is_transient(&SessionError::Hidraw(HidrawError::NotFound)));
    }

    #[test]
    fn one_stall_keeps_the_session_state_and_two_drop_it() {
        use hidpp::protocol::v20::Hidpp20Error;

        let dir = temp_dir("stalls");
        let mut daemon = daemon(&dir);
        daemon.state.connected = true;
        daemon.state.active_profile = Some(2);
        let stall = || SessionError::Request(Hidpp20Error::Channel(ChannelError::Timeout));

        daemon.handle_error(stall(), Some("rule 1"));
        assert!(daemon.state.connected);
        assert_eq!(daemon.state.active_profile, Some(2));
        assert!(daemon.rules_pending, "the rule is retried on the next tick");
        assert_eq!(daemon.device_error, None);

        daemon.handle_error(stall(), None);
        assert!(!daemon.state.connected);
        assert_eq!(daemon.state.active_profile, None);
        assert!(daemon.device_error.is_some());
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_broken_edit_keeps_the_last_good_rules() {
        let dir = temp_dir("config-watch");
        let path = dir.join("config.toml");
        std::fs::write(&path, "default_profile = 2\n").expect("write config");
        let mut watch = ConfigWatch::new(path.clone());
        assert_eq!(watch.config.default_profile, Some(2));

        std::fs::write(&path, "default_profile = \n").expect("write broken config");
        watch.reload();
        assert_eq!(watch.config.default_profile, Some(2));
        assert!(
            watch
                .error
                .as_deref()
                .is_some_and(|e| e.contains("config.toml"))
        );

        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn notices_saved_edits_once() {
        let dir = temp_dir("config-change");
        let path = dir.join("config.toml");
        let mut watch = ConfigWatch::new(path.clone());
        assert!(!watch.reload_if_changed(), "missing file is unchanged");

        std::fs::write(&path, "default_profile = 1\n").expect("write config");
        assert!(watch.reload_if_changed());
        assert!(!watch.reload_if_changed());
        assert_eq!(watch.config.default_profile, Some(1));

        std::fs::write(&path, "default_profile = 12\n").expect("rewrite config");
        assert!(watch.reload_if_changed());
        assert_eq!(watch.config.default_profile, Some(12));

        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn focus_events_update_state_and_report_real_changes() {
        let dir = temp_dir("focus");
        let mut daemon = daemon(&dir);

        assert!(daemon.focus_changed(Event::ActiveWindow {
            app: Some("cs2".to_owned())
        }));
        assert!(!daemon.focus_changed(Event::ActiveWindow {
            app: Some("cs2".to_owned())
        }));
        assert!(daemon.focus_changed(Event::FocusedMonitor {
            name: "DP-2".to_owned()
        }));
        assert_eq!(daemon.state.app.as_deref(), Some("cs2"));
        assert_eq!(daemon.state.monitor.as_deref(), Some("DP-2"));

        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn config_errors_take_precedence_in_published_state() {
        let dir = temp_dir("precedence");
        let mut daemon = daemon(&dir);
        daemon.rule_error = Some("rule 1: profile 3 is disabled".to_owned());
        daemon.device_error = Some("no supported Logitech device found".to_owned());
        assert_eq!(
            daemon.current_state().error.as_deref(),
            Some("no supported Logitech device found")
        );
        daemon.config.error = Some("config.toml is not valid".to_owned());
        assert_eq!(
            daemon.current_state().error.as_deref(),
            Some("config.toml is not valid")
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn publishes_atomically_and_only_on_change() {
        let dir = temp_dir("publish");
        let mut daemon = daemon(&dir);
        daemon.state.active_profile = Some(2);
        daemon.publish().expect("publish");

        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("state.json")).expect("state file"))
                .expect("state is JSON");
        assert_eq!(written["active_profile"], 2);
        assert!(!dir.join("state.json.tmp").exists());

        std::fs::remove_file(dir.join("state.json")).expect("remove state");
        daemon.publish().expect("unchanged publish");
        assert!(
            !dir.join("state.json").exists(),
            "unchanged state is not rewritten"
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
    }
}
