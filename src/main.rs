//! `omalogi`: configure Logitech G-series mice on Omarchy.

mod text;

use std::{
    error::Error,
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::{Parser, Subcommand};
use omalogi::{
    daemon,
    device::{CLI_SOFTWARE_ID, Session},
    editing::{BackupFile, ProfileChanges, save_backup},
    error_chain,
    lock::DeviceLock,
    onboard::{
        action::{catalog, parse_action},
        format::Binding,
    },
    rules::Config,
};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "omalogi", version, about)]
struct Cli {
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(flatten)]
    Device(DeviceCommand),
    /// List the actions buttons can be bound to, as accepted by `profiles edit --button`.
    Actions,
    /// Switch onboard profiles automatically as the focused app or monitor changes.
    Daemon {
        /// Rules file. Defaults to $XDG_CONFIG_HOME/omalogi/config.toml.
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DeviceCommand {
    /// Show the connected device, firmware, DPI and report rate.
    Info,
    /// List onboard profiles with their DPI stages and button bindings.
    Profiles {
        #[command(subcommand)]
        action: Option<ProfilesAction>,
    },
    /// Save all onboard profile memory to a JSON file.
    Backup {
        /// File to write; it must not exist yet. Defaults to $XDG_STATE_HOME/omalogi/backups/.
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Write profile memory back from a backup. The current memory is backed up first.
    Restore {
        /// A backup made by `omalogi backup` or saved before an edit.
        file: PathBuf,
        /// Show which sectors would be written, without writing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ProfilesAction {
    /// Make an enabled profile active. Numbers are as listed by `omalogi profiles`.
    Activate { number: usize },
    /// Change a profile's DPI stages, report rate or buttons.
    ///
    /// Profile memory is backed up to $XDG_STATE_HOME/omalogi/backups/ first, and the
    /// write is read back to verify it. Actions: left, right, middle, back, forward,
    /// button:N, dpi-up, dpi-down, dpi-cycle, dpi-default, dpi-shift, gshift,
    /// profile-next, profile-previous, profile-cycle, scroll-left, scroll-right,
    /// scroll-up, scroll-down, key:<combo> (e.g. key:ctrl+shift+t), media:<name>
    /// (volume-up, volume-down, mute, play-pause, next-track, previous-track), disabled.
    Edit {
        number: usize,
        /// DPI stages in order, e.g. 800,1600,3200 (up to 5).
        #[arg(long, value_delimiter = ',')]
        dpi: Option<Vec<u16>>,
        /// The DPI stage active after switching to the profile.
        #[arg(long)]
        default_dpi: Option<u16>,
        /// The DPI stage held with the DPI shift button.
        #[arg(long)]
        shift_dpi: Option<u16>,
        /// Report rate in Hz, e.g. 1000.
        #[arg(long)]
        rate: Option<u16>,
        /// A button binding as SLOT=ACTION, e.g. 6=key:ctrl+t. Repeat for more.
        #[arg(long = "button", value_name = "SLOT=ACTION", value_parser = parse_slot_action)]
        buttons: Vec<(usize, Binding)>,
        /// A G-Shift binding as SLOT=ACTION. Repeat for more.
        #[arg(long = "gshift", value_name = "SLOT=ACTION", value_parser = parse_slot_action)]
        gshift_buttons: Vec<(usize, Binding)>,
        /// Show the result, without writing.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("omalogi: could not start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = match cli.command {
        Command::Actions => print_actions(cli.json),
        Command::Daemon { config } => runtime.block_on(run_daemon(config)),
        Command::Device(command) => runtime.block_on(run_device(cli.json, command)),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("omalogi: {}", error_chain(error.as_ref()));
            ExitCode::FAILURE
        }
    }
}

fn print_actions(json: bool) -> Result<(), Box<dyn Error>> {
    let actions = catalog();
    output(json, &actions, || {
        actions.iter().fold(String::new(), |mut out, action| {
            out.push_str(&format!(
                "{:<9} {:<21} {}\n",
                action.group, action.value, action.label
            ));
            out
        })
    })?;
    Ok(())
}

async fn run_daemon(config: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    let path = match config {
        Some(path) => path,
        None => Config::default_path()
            .ok_or("could not determine the config directory; pass --config")?,
    };
    daemon::run(path).await?;
    Ok(())
}

async fn run_device(json: bool, command: DeviceCommand) -> Result<(), Box<dyn Error>> {
    // Read the backup before opening the device, so a bad file fails without touching it.
    let restore_file = match &command {
        DeviceCommand::Restore { file, .. } => Some(BackupFile::load(file)?),
        _ => None,
    };
    let mut session = Session::open(CLI_SOFTWARE_ID).await?;
    match command {
        DeviceCommand::Info => {
            let info = session.info().await?;
            output(json, &info, || text::info(&info))?;
        }
        DeviceCommand::Profiles { action: None } => {
            let state = session.onboard().await?;
            output(json, &state, || text::profiles(&state))?;
        }
        DeviceCommand::Profiles {
            action: Some(ProfilesAction::Activate { number }),
        } => {
            session.activate_profile(number).await?;
            #[derive(Serialize)]
            struct Activated {
                active_profile: usize,
            }
            output(
                json,
                &Activated {
                    active_profile: number,
                },
                || format!("Profile {number} is now active\n"),
            )?;
        }
        DeviceCommand::Profiles {
            action:
                Some(ProfilesAction::Edit {
                    number,
                    dpi,
                    default_dpi,
                    shift_dpi,
                    rate,
                    buttons,
                    gshift_buttons,
                    dry_run,
                }),
        } => {
            let changes = ProfileChanges {
                dpi_stages: dpi,
                default_dpi,
                shift_dpi,
                report_rate_hz: rate,
                buttons,
                gshift_buttons,
            };
            if changes.is_empty() {
                return Err("nothing to change; pass --dpi, --default-dpi, --shift-dpi, --rate, --button or --gshift".into());
            }
            if dry_run {
                let plan = session.plan_profile_changes(number, &changes).await?;
                output(json, &plan, || text::edit_plan(&plan))?;
            } else {
                let _lock = device_lock()?;
                let path = default_backup_path(session.model().name)?;
                let report = session
                    .apply_profile_changes(number, &changes, &path)
                    .await?;
                output(json, &report, || text::write_report(&report))?;
            }
        }
        DeviceCommand::Backup { output: path } => {
            let backup = session.backup().await?;
            let path = match path {
                Some(path) => path,
                None => default_backup_path(backup.device)?,
            };
            save_backup(&backup, &path)?;
            #[derive(Serialize)]
            struct Saved<'a> {
                path: &'a PathBuf,
                sectors: usize,
            }
            let saved = Saved {
                path: &path,
                sectors: backup.sectors.len(),
            };
            output(json, &saved, || {
                format!(
                    "Saved {} onboard memory sectors to {}\n",
                    saved.sectors,
                    path.display()
                )
            })?;
        }
        DeviceCommand::Restore { dry_run, .. } => {
            let backup = restore_file.expect("loaded above");
            if dry_run {
                let plan = session.plan_restore(&backup).await?;
                output(json, &plan, || text::restore_plan(&plan))?;
            } else {
                let _lock = device_lock()?;
                let path = default_backup_path(session.model().name)?;
                let report = session.restore(&backup, &path).await?;
                output(json, &report, || text::restore_report(&report))?;
            }
        }
    }
    Ok(())
}

/// Held for a whole memory write or restore, so the daemon never polls in the middle.
fn device_lock() -> Result<Option<DeviceLock>, Box<dyn Error>> {
    let Some(path) = DeviceLock::default_path() else {
        return Ok(None);
    };
    let lock = DeviceLock::acquire(&path)
        .map_err(|error| format!("could not lock {}: {error}", path.display()))?;
    Ok(Some(lock))
}

fn parse_slot_action(text: &str) -> Result<(usize, Binding), String> {
    let (slot, action) = text
        .split_once('=')
        .ok_or_else(|| format!("`{text}`: use SLOT=ACTION, e.g. 6=key:ctrl+t"))?;
    let slot = slot
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("`{slot}` is not a slot number"))?;
    Ok((slot, parse_action(action)?))
}

fn output<T: Serialize>(
    json: bool,
    value: &T,
    text: impl FnOnce() -> String,
) -> Result<(), serde_json::Error> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        print!("{}", text());
    }
    Ok(())
}

fn default_backup_path(device: &str) -> Result<PathBuf, Box<dyn Error>> {
    let state_home = match std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir()
            .ok_or("could not determine the home directory; pass --output")?
            .join(".local/state"),
    };
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let slug = device.to_lowercase().replace(' ', "-");
    Ok(state_home
        .join("omalogi/backups")
        .join(format!("{slug}-{millis}.json")))
}
