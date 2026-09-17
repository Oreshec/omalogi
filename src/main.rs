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
    assets, daemon,
    device::{CLI_SOFTWARE_ID, Session},
    editing::{BackupFile, DirectoryRepair, ProfileChanges, save_backup},
    error_chain,
    hidraw::{HidrawError, find_supported},
    lock::DeviceLock,
    onboard::{
        action::{catalog, parse_action},
        format::Binding,
    },
    rules::Config,
    serve, setup,
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
    /// Set Omalogi up for your user: install the shell plugin, put its indicator on the
    /// bar, and enable the daemon. Needs no root; run it again after every upgrade.
    Setup {
        /// Show what would change, without changing anything.
        #[arg(long)]
        dry_run: bool,
        /// Leave the bar as it is.
        #[arg(long)]
        no_bar: bool,
        /// Do not enable the automatic switching daemon.
        #[arg(long)]
        no_daemon: bool,
    },
    /// Show where the connected mouse's picture is cached, with each button's position.
    ///
    /// The picture is downloaded once from assets.openlogi.org, checked against the
    /// host's checksums, and cached in $XDG_CACHE_HOME/omalogi/pictures. It does not
    /// talk to the mouse, so it can run alongside other commands.
    Picture {
        /// Only use the cache; never download.
        #[arg(long, conflicts_with = "refresh")]
        offline: bool,
        /// Check the host for an updated picture.
        #[arg(long)]
        refresh: bool,
    },
    /// Serve the shell plugin: JSON requests on stdin, one per line, answered on stdout.
    #[command(hide = true)]
    Serve,
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
    /// Accept editing a mouse Omalogi has not been tested on yet. Every write is still
    /// backed up and verified; please report how it went.
    AcceptUntested,
    /// Show the sensor's live DPI, or set it without saving it to any profile.
    Dpi {
        /// The DPI to use right now, e.g. 1600.
        value: Option<u16>,
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
    /// Turn a profile on, so the mouse can switch to it. Profile memory is backed up first.
    Enable {
        number: usize,
        /// Check the change, without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Turn a profile off. The profile in use and the last one turned on cannot be.
    Disable {
        number: usize,
        /// Check the change, without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Rebuild a profile directory whose checksum does not match from its own entries.
    ///
    /// Only the directory is written, and only when its entries are consistent and every
    /// profile it lists passes its own checksum. Profile memory is backed up first.
    Repair {
        /// Check the directory and show the result, without writing.
        #[arg(long)]
        dry_run: bool,
    },
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
        /// Profile name, up to 47 printable ASCII characters; an empty name clears it.
        #[arg(long)]
        name: Option<String>,
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
        Command::Serve => runtime.block_on(run_serve()),
        Command::Picture { offline, refresh } => {
            runtime.block_on(show_picture(cli.json, assets::Options { offline, refresh }))
        }
        Command::Setup {
            dry_run,
            no_bar,
            no_daemon,
        } => run_setup(
            cli.json,
            setup::Options {
                dry_run,
                bar: !no_bar,
                daemon: !no_daemon,
            },
        ),
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

async fn show_picture(json: bool, options: assets::Options) -> Result<(), Box<dyn Error>> {
    let model = match find_supported() {
        Ok(node) => node.device,
        // No wired mouse: a known mouse behind a receiver, found through OpenLogi.
        Err(HidrawError::NotFound) => omalogi::wireless::find()
            .await?
            .into_iter()
            .next()
            .map(|mouse| mouse.model)
            .ok_or(HidrawError::NotFound)?,
        Err(error) => return Err(error.into()),
    };
    let picture = assets::picture(model.product_id, options)?;
    output(json, &picture, || {
        let mut out = format!("{} picture ({}):\n", model.name, picture.depot);
        for view in &picture.views {
            out.push_str(&format!(
                "  {:<5}  {}  ({} button positions)\n",
                view.name,
                view.image.display(),
                view.hotspots.len()
            ));
        }
        if !picture.slots_verified {
            out.push_str(
                "  Button positions are not verified for this device, so none are shown.\n",
            );
        }
        out
    })?;
    Ok(())
}

async fn run_serve() -> Result<(), Box<dyn Error>> {
    // The overlay starts this on demand; ride out a single stalled first request, as the
    // daemon does, before reporting the device as unavailable.
    let session = match open_session(serve::SOFTWARE_ID).await {
        Ok(session) => session,
        Err(_) => {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            open_session(serve::SOFTWARE_ID).await?
        }
    };
    let server = serve::Server::new(session, DeviceLock::default_path(), default_backup_path);
    server
        .run(
            tokio::io::BufReader::new(tokio::io::stdin()),
            tokio::io::stdout(),
        )
        .await?;
    Ok(())
}

fn run_setup(json: bool, options: setup::Options) -> Result<(), Box<dyn Error>> {
    let report = setup::setup(options);
    output(json, &report, || {
        let mut out = String::from(if report.dry_run {
            "Setup (dry run, nothing changed):\n"
        } else {
            "Setup:\n"
        });
        for step in &report.steps {
            let status = match step.status {
                setup::Status::Done => "done",
                setup::Status::UpToDate => "current",
                setup::Status::Planned => "planned",
                setup::Status::Skipped => "skipped",
                setup::Status::Warning => "warning",
                setup::Status::Failed => "failed",
            };
            out.push_str(&format!("  {status:<8} {:<7} {}\n", step.name, step.detail));
        }
        out
    })?;
    if report.failed() {
        return Err("setup did not finish; see the failed step above".into());
    }
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
    // Held for the whole command, so no other Omalogi process's requests interleave with
    // this one's (see `open_session`).
    let _device = device_lock()?;
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
                Some(
                    action @ (ProfilesAction::Enable { number, dry_run }
                    | ProfilesAction::Disable { number, dry_run }),
                ),
        } => {
            let enabled = matches!(action, ProfilesAction::Enable { .. });
            let turned = if enabled { "on" } else { "off" };
            // Refuse before the backup, so a no-op or a refused change writes nothing at all.
            session.check_profile_enabled(number, enabled).await?;
            if dry_run {
                #[derive(Serialize)]
                struct Planned {
                    profile: usize,
                    enabled: bool,
                    dry_run: bool,
                }
                let planned = Planned {
                    profile: number,
                    enabled,
                    dry_run,
                };
                output(json, &planned, || {
                    format!("Profile {number} would be turned {turned}; nothing was written\n")
                })?;
                return Ok(());
            }
            let path = default_backup_path(session.model().name)?;
            save_backup(&session.backup().await?, &path)?;
            session.set_profile_enabled(number, enabled).await?;
            let state = session.onboard().await?;
            #[derive(Serialize)]
            struct Toggled<'a> {
                profile: usize,
                enabled: bool,
                backup: &'a PathBuf,
            }
            output(
                json,
                &Toggled {
                    profile: number,
                    enabled,
                    backup: &path,
                },
                || {
                    format!(
                        "Profile {number} is now turned {}\nBackup saved to {}\n\n{}",
                        if enabled { "on" } else { "off" },
                        path.display(),
                        text::profiles(&state)
                    )
                },
            )?;
        }
        DeviceCommand::Profiles {
            action: Some(ProfilesAction::Repair { dry_run }),
        } => {
            // Checked before the backup, so an intact or unrepairable directory writes nothing.
            let plan = session.plan_directory_repair().await?;
            if dry_run {
                output(json, &plan, || text::directory_repair(&plan, true))?;
                return Ok(());
            }
            let path = default_backup_path(session.model().name)?;
            let backup = session.backup().await?;
            save_backup(&backup, &path)?;
            let repair = session.repair_directory().await?;
            let state = session.onboard().await?;
            #[derive(Serialize)]
            struct Repaired<'a> {
                #[serde(flatten)]
                repair: &'a DirectoryRepair,
                backup: &'a PathBuf,
            }
            output(
                json,
                &Repaired {
                    repair: &repair,
                    backup: &path,
                },
                || {
                    format!(
                        "{}Backup of the memory before repairing: {}\n\n{}",
                        text::directory_repair(&repair, false),
                        path.display(),
                        text::profiles(&state)
                    )
                },
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
                    name,
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
                name,
            };
            if changes.is_empty() {
                return Err("nothing to change; pass --dpi, --default-dpi, --shift-dpi, --rate, --name, --button or --gshift".into());
            }
            if dry_run {
                let plan = session.plan_profile_changes(number, &changes).await?;
                output(json, &plan, || text::edit_plan(&plan))?;
            } else {
                let path = default_backup_path(session.model().name)?;
                let report = session
                    .apply_profile_changes(number, &changes, &path)
                    .await?;
                output(json, &report, || text::write_report(&report))?;
            }
        }
        DeviceCommand::AcceptUntested => {
            let support = session.accept_untested().await?;
            output(json, &support, || {
                if support.verified {
                    format!("The {} is verified; it needs no acceptance\n", support.name)
                } else {
                    format!(
                        "You can now edit the {}. It has not been tested with Omalogi, so every \
                         write is backed up first and read back to verify it.\nTell us how it \
                         went: https://github.com/elberacasa/omalogi/issues/new?template=device.yml\n",
                        support.name
                    )
                }
            })?;
        }
        DeviceCommand::Dpi { value } => {
            let dpi = match value {
                None => session.live_dpi().await?,
                Some(value) => {
                    if !session.info().await?.dpi_values.contains(&value) {
                        return Err(format!("the sensor does not support {value} DPI").into());
                    }
                    session.set_live_dpi(value).await?
                }
            };
            #[derive(Serialize)]
            struct LiveDpi {
                dpi: u16,
            }
            output(json, &LiveDpi { dpi }, || format!("{dpi} DPI\n"))?;
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
                #[serde(skip_serializing_if = "<[String]>::is_empty")]
                invalid_checksums: &'a [String],
            }
            let saved = Saved {
                path: &path,
                sectors: backup.sectors.len(),
                invalid_checksums: &backup.invalid_checksums,
            };
            output(json, &saved, || {
                format!(
                    "Saved {} onboard memory sectors to {}\n{}",
                    saved.sectors,
                    path.display(),
                    text::invalid_checksums(saved.invalid_checksums)
                )
            })?;
        }
        DeviceCommand::Restore { dry_run, .. } => {
            let backup = restore_file.expect("loaded above");
            if dry_run {
                let plan = session.plan_restore(&backup).await?;
                output(json, &plan, || text::restore_plan(&plan))?;
            } else {
                let path = default_backup_path(session.model().name)?;
                let report = session.restore(&backup, &path).await?;
                output(json, &report, || text::restore_report(&report))?;
            }
        }
    }
    Ok(())
}

/// Opens the device with the device lock held while connecting, for `omalogi serve`,
/// which then takes the lock per request.
///
/// Requests from two Omalogi processes at the same moment are not safe on a G502 X: 6 of
/// 10 overlapping server starts timed out for 5.5 s, and a memory read overlapping
/// another process's reads came back with the wrong bytes. Every process therefore holds
/// the device lock while it talks to the mouse; the daemon skips a poll while it is taken.
async fn open_session(software_id: u8) -> Result<Session, Box<dyn Error>> {
    let _connecting = device_lock()?;
    Ok(Session::open(software_id).await?)
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
