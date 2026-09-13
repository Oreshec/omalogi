//! `omalogi`: configure Logitech G-series mice on Omarchy.

mod text;

use std::{
    error::Error,
    fs,
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::{Parser, Subcommand};
use omalogi::{
    daemon,
    device::{CLI_SOFTWARE_ID, Session},
    error_chain,
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
        /// File to write. Defaults to $XDG_STATE_HOME/omalogi/backups/.
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ProfilesAction {
    /// Make an enabled profile active. Numbers are as listed by `omalogi profiles`.
    Activate { number: usize },
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
        DeviceCommand::Backup { output: path } => {
            let backup = session.backup().await?;
            let path = match path {
                Some(path) => path,
                None => default_backup_path(backup.device)?,
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, serde_json::to_string_pretty(&backup)? + "\n")?;
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
    }
    Ok(())
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
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let slug = device.to_lowercase().replace(' ', "-");
    Ok(state_home
        .join("omalogi/backups")
        .join(format!("{slug}-{seconds}.json")))
}
