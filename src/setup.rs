//! `omalogi setup`: installs the shell plugin, puts its indicator on the bar and enables
//! the daemon, for the current user. Nothing here needs root: the udev rule and the
//! packaged systemd unit come from the package.

use std::{
    env,
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;
use serde_json::Value;

use crate::hidraw::{self, HidrawError};

pub const PLUGIN_ID: &str = "io.github.elberacasa.omalogi";
const UNIT_NAME: &str = "omalogi.service";
const PACKAGED_UNIT: &str = "/usr/lib/systemd/user/omalogi.service";
const UDEV_RULE: &str = "/usr/lib/udev/rules.d/70-omalogi.rules";
const UNIT_TEMPLATE: &str = include_str!("../packaging/systemd/omalogi.service");
const PACKAGED_EXEC: &str = "ExecStart=/usr/bin/omalogi daemon";

/// The shell plugin, built into the binary so the installed plugin always matches it.
pub const PLUGIN_FILES: &[(&str, &str)] = &[
    ("manifest.json", include_str!("../manifest.json")),
    ("plugin/Omalogi.qml", include_str!("../plugin/Omalogi.qml")),
    (
        "plugin/OmalogiCommand.qml",
        include_str!("../plugin/OmalogiCommand.qml"),
    ),
    (
        "plugin/Indicator.qml",
        include_str!("../plugin/Indicator.qml"),
    ),
    ("plugin/Model.js", include_str!("../plugin/Model.js")),
    (
        "plugin/DpiTrack.qml",
        include_str!("../plugin/DpiTrack.qml"),
    ),
    ("plugin/Icons.js", include_str!("../plugin/Icons.js")),
    ("plugin/Icon.qml", include_str!("../plugin/Icon.qml")),
    (
        "plugin/OmalogiServer.qml",
        include_str!("../plugin/OmalogiServer.qml"),
    ),
    (
        "plugin/DeviceCanvas.qml",
        include_str!("../plugin/DeviceCanvas.qml"),
    ),
    (
        "plugin/ActionLibrary.qml",
        include_str!("../plugin/ActionLibrary.qml"),
    ),
    (
        "plugin/SensitivityPanel.qml",
        include_str!("../plugin/SensitivityPanel.qml"),
    ),
    (
        "plugin/PixelMark.qml",
        include_str!("../plugin/PixelMark.qml"),
    ),
];

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub dry_run: bool,
    pub bar: bool,
    pub daemon: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Done,
    UpToDate,
    Planned,
    Skipped,
    Warning,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub dry_run: bool,
    pub steps: Vec<Step>,
}

impl Report {
    #[must_use]
    pub fn failed(&self) -> bool {
        self.steps.iter().any(|step| step.status == Status::Failed)
    }

    fn push(&mut self, name: &'static str, status: Status, detail: impl Into<String>) {
        self.steps.push(Step {
            name,
            status,
            detail: detail.into(),
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Created,
    Updated,
    UpToDate,
}

/// Writes the plugin files into `dir`, leaving files that already match untouched.
/// Files are written to a temporary name and renamed, so the shell never sees half a file.
pub fn install_plugin(dir: &Path, dry_run: bool) -> io::Result<Vec<(PathBuf, FileState)>> {
    let mut states = Vec::with_capacity(PLUGIN_FILES.len());
    for (relative, contents) in PLUGIN_FILES {
        let path = dir.join(relative);
        let state = match fs::read(&path) {
            Ok(existing) if existing == contents.as_bytes() => FileState::UpToDate,
            Ok(_) => FileState::Updated,
            Err(error) if error.kind() == io::ErrorKind::NotFound => FileState::Created,
            Err(error) => return Err(error),
        };
        if !dry_run && state != FileState::UpToDate {
            write_atomically(&path, contents.as_bytes())?;
        }
        states.push((path, state));
    }
    Ok(states)
}

fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = parent.join(format!(".{name}.omalogi-tmp"));
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, path)
}

/// Where the shell config currently has Omalogi.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShellPlacement {
    /// Listed in `plugins[]`, where `omarchy bar put` silently refuses to place it.
    pub listed_as_plugin: bool,
    pub on_bar: bool,
}

/// Reads Omalogi's placement from omarchy-shell's `shell.json`.
#[must_use]
pub fn shell_placement(config: &Value) -> ShellPlacement {
    let has_id = |entries: &Value| {
        entries.as_array().is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.as_str().or_else(|| entry["id"].as_str()) == Some(PLUGIN_ID))
        })
    };
    ShellPlacement {
        listed_as_plugin: has_id(&config["plugins"]),
        on_bar: config["bar"]["layout"]
            .as_object()
            .is_some_and(|sections| sections.values().any(has_id)),
    }
}

/// The systemd user unit for a non-packaged install, running `command`.
#[must_use]
pub fn user_unit(command: &Path) -> String {
    let command = command.display().to_string();
    let command = if command.chars().any(char::is_whitespace) {
        format!("\"{command}\"")
    } else {
        command
    };
    UNIT_TEMPLATE.replace(PACKAGED_EXEC, &format!("ExecStart={command} daemon"))
}

/// How the user runs omalogi: the PATH entry that resolves to this binary, so a unit
/// keeps working when a symlink is repointed; otherwise the binary itself.
fn command_path() -> io::Result<PathBuf> {
    let exe = env::current_exe()?;
    let canonical = fs::canonicalize(&exe).unwrap_or_else(|_| exe.clone());
    let on_path = env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|dir| dir.join("omalogi"))
            .find(|candidate| fs::canonicalize(candidate).is_ok_and(|c| c == canonical))
    });
    Ok(on_path.unwrap_or(exe))
}

fn config_home() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".config")))
}

/// Runs a command, returning its trimmed stdout or a one-line reason it failed.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program).args(args).output().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!("`{program}` is not installed")
        } else {
            format!("could not run `{program}`: {error}")
        }
    })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr.lines().rev().find(|line| !line.trim().is_empty());
    Err(format!(
        "`{program} {}` failed: {}",
        args.join(" "),
        reason.unwrap_or("no error output")
    ))
}

pub fn setup(options: Options) -> Report {
    let mut report = Report {
        dry_run: options.dry_run,
        steps: Vec::new(),
    };
    let Some(config_home) = config_home() else {
        report.push(
            "plugin",
            Status::Failed,
            "could not determine the config directory; set XDG_CONFIG_HOME",
        );
        return report;
    };
    if !plugin_step(&mut report, &config_home, options.dry_run) {
        return report;
    }
    if options.bar {
        bar_step(&mut report, &config_home, options.dry_run);
    } else {
        report.push("bar", Status::Skipped, "--no-bar");
    }
    if options.daemon {
        daemon_step(&mut report, &config_home, options.dry_run);
    } else {
        report.push("daemon", Status::Skipped, "--no-daemon");
    }
    access_step(&mut report);
    report
}

/// Returns false when the plugin could not be installed, which stops the setup.
fn plugin_step(report: &mut Report, config_home: &Path, dry_run: bool) -> bool {
    let dir = config_home.join("omarchy/plugins").join(PLUGIN_ID);
    let existed = dir.exists();
    let files = match install_plugin(&dir, dry_run) {
        Ok(files) => files,
        Err(error) => {
            report.push(
                "plugin",
                Status::Failed,
                format!("could not write {}: {error}", dir.display()),
            );
            return false;
        }
    };
    let changed = files
        .iter()
        .filter(|(_, state)| *state != FileState::UpToDate)
        .count();
    if changed == 0 {
        report.push(
            "plugin",
            Status::UpToDate,
            format!("{} is up to date", dir.display()),
        );
        return true;
    }
    let verb = if dry_run { "would write" } else { "wrote" };
    let detail = format!(
        "{verb} {changed} of {} files in {}",
        files.len(),
        dir.display()
    );
    if dry_run {
        report.push("plugin", Status::Planned, detail);
        return true;
    }
    report.push("plugin", Status::Done, detail);
    // A running shell keeps the overlay it already loaded, even after a rescan: Omarchy
    // opens a bar-widget plugin's overlay through the widget, which a rescan does not
    // rebuild. A first install only needs the rescan to be discovered.
    if existed {
        report.push(
            "shell",
            Status::Warning,
            "run `omarchy restart shell` to load the new version",
        );
    } else if let Err(reason) = run("omarchy-shell", &["-q", "shell", "rescanPlugins"]) {
        report.push("shell", Status::Warning, reason);
    }
    true
}

fn bar_step(report: &mut Report, config_home: &Path, dry_run: bool) {
    let path = config_home.join("omarchy/shell.json");
    let placement = match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(config) => shell_placement(&config),
            Err(error) => {
                report.push(
                    "bar",
                    Status::Failed,
                    format!("could not parse {}: {error}", path.display()),
                );
                return;
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => ShellPlacement::default(),
        Err(error) => {
            report.push(
                "bar",
                Status::Failed,
                format!("could not read {}: {error}", path.display()),
            );
            return;
        }
    };
    if placement.on_bar {
        report.push("bar", Status::UpToDate, "the indicator is on the bar");
        return;
    }
    if dry_run {
        report.push(
            "bar",
            Status::Planned,
            "would put the indicator in the bar's right section",
        );
        return;
    }
    // Omarchy enables a bar widget by placing it; `bar put` answers "ok" without placing
    // a widget that is still enabled as a plain plugin.
    if placement.listed_as_plugin
        && let Err(reason) = run("omarchy", &["plugin", "disable", PLUGIN_ID])
    {
        report.push("bar", Status::Failed, reason);
        return;
    }
    match run("omarchy", &["bar", "put", PLUGIN_ID, "--section", "right"]) {
        Ok(_) => report.push(
            "bar",
            Status::Done,
            "put the indicator in the bar's right section",
        ),
        Err(reason) => report.push("bar", Status::Failed, reason),
    }
}

fn daemon_step(report: &mut Report, config_home: &Path, dry_run: bool) {
    let user_path = config_home.join("systemd/user").join(UNIT_NAME);
    let packaged = Path::new(PACKAGED_UNIT).exists();
    let mut wrote_unit = false;
    if user_path.exists() {
        if packaged {
            report.push(
                "daemon",
                Status::Warning,
                format!(
                    "{} overrides the packaged unit; remove it to run /usr/bin/omalogi",
                    user_path.display()
                ),
            );
        }
    } else if !packaged {
        let command = match command_path() {
            Ok(command) => command,
            Err(error) => {
                report.push(
                    "daemon",
                    Status::Failed,
                    format!("could not find the omalogi binary: {error}"),
                );
                return;
            }
        };
        if dry_run {
            report.push(
                "daemon",
                Status::Planned,
                format!(
                    "would write {} running {}",
                    user_path.display(),
                    command.display()
                ),
            );
        } else if let Err(error) = write_atomically(&user_path, user_unit(&command).as_bytes()) {
            report.push(
                "daemon",
                Status::Failed,
                format!("could not write {}: {error}", user_path.display()),
            );
            return;
        } else {
            wrote_unit = true;
        }
    }
    if dry_run {
        report.push(
            "daemon",
            Status::Planned,
            "would enable and restart omalogi.service",
        );
        return;
    }
    let commands: &[&[&str]] = if wrote_unit {
        &[
            &["--user", "daemon-reload"],
            &["--user", "enable", UNIT_NAME],
            &["--user", "restart", UNIT_NAME],
        ]
    } else {
        &[
            &["--user", "enable", UNIT_NAME],
            &["--user", "restart", UNIT_NAME],
        ]
    };
    for args in commands {
        if let Err(reason) = run("systemctl", args) {
            report.push("daemon", Status::Failed, reason);
            return;
        }
    }
    // A user unit, written now or earlier, takes precedence over the packaged one.
    let unit = if user_path.exists() {
        user_path.display().to_string()
    } else {
        PACKAGED_UNIT.to_owned()
    };
    report.push(
        "daemon",
        Status::Done,
        format!("enabled and restarted omalogi.service ({unit})"),
    );
}

/// Opens the HID++ node read-write without sending anything, to check permissions.
fn access_step(report: &mut Report) {
    let node = match hidraw::find_supported() {
        Ok(node) => node,
        Err(HidrawError::NotFound) => {
            report.push(
                "device",
                Status::Skipped,
                "no supported mouse connected; plug in the G502 X to check access",
            );
            return;
        }
        Err(error) => {
            report.push("device", Status::Warning, crate::error_chain(&error));
            return;
        }
    };
    let path = node.path.display();
    match OpenOptions::new().read(true).write(true).open(&node.path) {
        Ok(_) => report.push(
            "device",
            Status::Done,
            format!("{} at {path} is accessible", node.device.name),
        ),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            let fix = if Path::new(UDEV_RULE).exists() {
                "replug the mouse so the udev rule applies".to_owned()
            } else {
                format!(
                    "install the udev rule (packaging/udev/70-omalogi.rules) to {UDEV_RULE}, \
                     reload udev, then replug the mouse"
                )
            };
            report.push(
                "device",
                Status::Warning,
                format!("no access to {path}; {fix}"),
            );
        }
        Err(error) => report.push(
            "device",
            Status::Warning,
            format!("could not open {path}: {error}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("omalogi-setup-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn states(files: &[(PathBuf, FileState)]) -> Vec<FileState> {
        files.iter().map(|(_, state)| *state).collect()
    }

    #[test]
    fn plugin_install_creates_then_leaves_matching_files() {
        let dir = TempDir::new("install");
        let first = install_plugin(&dir.0, false).unwrap();
        assert_eq!(states(&first), vec![FileState::Created; PLUGIN_FILES.len()]);
        for (relative, contents) in PLUGIN_FILES {
            assert_eq!(fs::read_to_string(dir.0.join(relative)).unwrap(), *contents);
        }
        let second = install_plugin(&dir.0, false).unwrap();
        assert_eq!(
            states(&second),
            vec![FileState::UpToDate; PLUGIN_FILES.len()]
        );
        assert!(fs::read_dir(dir.0.join("plugin")).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with("-tmp")
        }));
    }

    #[test]
    fn plugin_install_updates_changed_files() {
        let dir = TempDir::new("update");
        install_plugin(&dir.0, false).unwrap();
        fs::write(dir.0.join("plugin/Model.js"), "stale").unwrap();
        let files = install_plugin(&dir.0, false).unwrap();
        let model = files
            .iter()
            .find(|(path, _)| path.ends_with("plugin/Model.js"))
            .unwrap();
        assert_eq!(model.1, FileState::Updated);
        assert_ne!(fs::read_to_string(&model.0).unwrap(), "stale");
    }

    #[test]
    fn every_plugin_file_is_built_in() {
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin");
        for entry in fs::read_dir(&plugin).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                continue;
            }
            let relative = format!("plugin/{}", entry.file_name().to_string_lossy());
            assert!(
                PLUGIN_FILES.iter().any(|(path, _)| *path == relative),
                "{relative} is not in PLUGIN_FILES"
            );
        }
    }

    #[test]
    fn plugin_dry_run_writes_nothing() {
        let dir = TempDir::new("dry-run");
        let files = install_plugin(&dir.0, true).unwrap();
        assert_eq!(states(&files), vec![FileState::Created; PLUGIN_FILES.len()]);
        assert!(!dir.0.exists());
    }

    #[test]
    fn built_in_plugin_covers_the_manifest_entry_points() {
        let manifest: Value = serde_json::from_str(PLUGIN_FILES[0].1).unwrap();
        assert_eq!(manifest["id"], PLUGIN_ID);
        let entry_points = manifest["entryPoints"].as_object().unwrap();
        assert!(!entry_points.is_empty());
        for entry in entry_points.values() {
            let entry = entry.as_str().unwrap();
            assert!(
                PLUGIN_FILES.iter().any(|(relative, _)| *relative == entry),
                "{entry} is not built in"
            );
        }
    }

    #[test]
    fn placement_reads_plugins_and_bar_layout() {
        assert_eq!(shell_placement(&json!({})), ShellPlacement::default());
        let listed = json!({ "plugins": [{ "id": "other" }, PLUGIN_ID] });
        assert_eq!(
            shell_placement(&listed),
            ShellPlacement {
                listed_as_plugin: true,
                on_bar: false
            }
        );
        let placed = json!({
            "plugins": [{ "id": "io.github.elberacasa.omahub" }],
            "bar": { "layout": { "left": ["omarchy.menu"], "right": [{ "id": PLUGIN_ID }] } }
        });
        assert_eq!(
            shell_placement(&placed),
            ShellPlacement {
                listed_as_plugin: false,
                on_bar: true
            }
        );
    }

    #[test]
    fn user_unit_runs_the_given_binary() {
        let unit = user_unit(Path::new("/home/me/.local/bin/omalogi"));
        assert!(unit.contains("\nExecStart=/home/me/.local/bin/omalogi daemon\n"));
        assert!(!unit.contains("/usr/bin/omalogi"));
        assert!(unit.contains("WantedBy=graphical-session.target"));
        let spaced = user_unit(Path::new("/home/me/my bin/omalogi"));
        assert!(spaced.contains("\nExecStart=\"/home/me/my bin/omalogi\" daemon\n"));
    }
}
