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
use omalogi::device::Session;
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
    match runtime.block_on(run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("omalogi: {}", error_chain(error.as_ref()));
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let mut session = Session::open().await?;
    match cli.command {
        Command::Info => {
            let info = session.info().await?;
            output(cli.json, &info, || text::info(&info))?;
        }
        Command::Profiles { action: None } => {
            let state = session.onboard().await?;
            output(cli.json, &state, || text::profiles(&state))?;
        }
        Command::Profiles {
            action: Some(ProfilesAction::Activate { number }),
        } => {
            session.activate_profile(number).await?;
            #[derive(Serialize)]
            struct Activated {
                active_profile: usize,
            }
            output(
                cli.json,
                &Activated {
                    active_profile: number,
                },
                || format!("Profile {number} is now active\n"),
            )?;
        }
        Command::Backup { output: path } => {
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
            output(cli.json, &saved, || {
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

fn error_chain(error: &dyn Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
