//! Hyprland IPC: focus events from `.socket2.sock` and focus queries on `.socket.sock`.
//!
//! Event layouts are from Hyprland v0.56.2 `src/desktop/state/FocusState.cpp`:
//! `activewindow>>CLASS,TITLE` (`,` when nothing is focused) and
//! `focusedmon>>MONNAME,WORKSPACENAME`. Titles are not escaped, so data is split at
//! the first comma, as Quickshell's Hyprland IPC does.

use std::{io, path::PathBuf};

use serde_json::Value;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines},
    net::UnixStream,
};

use crate::rules::Focus;

#[derive(Debug, Error)]
pub enum HyprlandError {
    #[error("HYPRLAND_INSTANCE_SIGNATURE is not set; run this inside a Hyprland session")]
    NotRunning,
    #[error("could not use the Hyprland socket {path}")]
    Socket {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Hyprland gave an unexpected reply to {command}")]
    BadReply {
        command: &'static str,
        #[source]
        source: serde_json::Error,
    },
}

/// A focus change Omalogi cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The focused window's class, or `None` when no window is focused.
    ActiveWindow {
        app: Option<String>,
    },
    FocusedMonitor {
        name: String,
    },
}

/// Parses one line from the event socket; other events are `None`.
#[must_use]
pub fn parse_event(line: &str) -> Option<Event> {
    let (name, data) = line.split_once(">>")?;
    let first_field = data.split_once(',').map_or(data, |(field, _)| field);
    match name {
        "activewindow" => Some(Event::ActiveWindow {
            app: (!first_field.is_empty()).then(|| first_field.to_owned()),
        }),
        "focusedmon" if !first_field.is_empty() => Some(Event::FocusedMonitor {
            name: first_field.to_owned(),
        }),
        _ => None,
    }
}

fn socket_path(name: &str) -> Result<PathBuf, HyprlandError> {
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .filter(|value| !value.is_empty())
        .ok_or(HyprlandError::NotRunning)?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .ok_or(HyprlandError::NotRunning)?;
    Ok(PathBuf::from(runtime)
        .join("hypr")
        .join(signature)
        .join(name))
}

async fn connect(name: &str) -> Result<(UnixStream, String), HyprlandError> {
    let path = socket_path(name)?;
    let display = path.display().to_string();
    let stream = UnixStream::connect(&path)
        .await
        .map_err(|source| HyprlandError::Socket {
            path: display.clone(),
            source,
        })?;
    Ok((stream, display))
}

/// The stream of focus events.
pub struct EventStream {
    lines: Lines<BufReader<UnixStream>>,
    path: String,
}

impl EventStream {
    pub async fn connect() -> Result<Self, HyprlandError> {
        let (stream, path) = connect(".socket2.sock").await?;
        Ok(Self {
            lines: BufReader::new(stream).lines(),
            path,
        })
    }

    /// The next focus event, or `None` when Hyprland closes the socket. Cancel-safe.
    pub async fn next(&mut self) -> Result<Option<Event>, HyprlandError> {
        loop {
            let line = self
                .lines
                .next_line()
                .await
                .map_err(|source| HyprlandError::Socket {
                    path: self.path.clone(),
                    source,
                })?;
            match line {
                None => return Ok(None),
                Some(line) => {
                    if let Some(event) = parse_event(&line) {
                        return Ok(Some(event));
                    }
                }
            }
        }
    }
}

async fn request(command: &'static str) -> Result<Value, HyprlandError> {
    let (mut stream, path) = connect(".socket.sock").await?;
    let io_error = |source| HyprlandError::Socket {
        path: path.clone(),
        source,
    };
    stream
        .write_all(command.as_bytes())
        .await
        .map_err(io_error)?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).await.map_err(io_error)?;
    serde_json::from_slice(&reply).map_err(|source| HyprlandError::BadReply { command, source })
}

/// The focused window's class and the focused monitor, as Hyprland reports them now.
pub async fn current_focus() -> Result<Focus, HyprlandError> {
    let window = request("j/activewindow").await?;
    let monitors = request("j/monitors").await?;
    Ok(focus_from(&window, &monitors))
}

fn focus_from(window: &Value, monitors: &Value) -> Focus {
    let app = window
        .get("class")
        .and_then(Value::as_str)
        .filter(|class| !class.is_empty())
        .map(str::to_owned);
    let monitor = monitors
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|monitor| monitor.get("focused").and_then(Value::as_bool) == Some(true))
        })
        .and_then(|monitor| monitor.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Focus { app, monitor }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_active_window_class() {
        assert_eq!(
            parse_event("activewindow>>cursor,omarchy-logitech - Cursor"),
            Some(Event::ActiveWindow {
                app: Some("cursor".to_owned())
            })
        );
    }

    #[test]
    fn titles_with_commas_do_not_change_the_class() {
        assert_eq!(
            parse_event("activewindow>>firefox,Inbox (3), Mail, and more"),
            Some(Event::ActiveWindow {
                app: Some("firefox".to_owned())
            })
        );
    }

    #[test]
    fn nothing_focused_clears_the_app() {
        assert_eq!(
            parse_event("activewindow>>,"),
            Some(Event::ActiveWindow { app: None })
        );
    }

    #[test]
    fn parses_focused_monitor() {
        assert_eq!(
            parse_event("focusedmon>>DP-2,3"),
            Some(Event::FocusedMonitor {
                name: "DP-2".to_owned()
            })
        );
        assert_eq!(parse_event("focusedmon>>,1"), None);
    }

    #[test]
    fn ignores_other_events_and_noise() {
        assert_eq!(parse_event("activewindowv2>>55d1c0a2b3c0"), None);
        assert_eq!(parse_event("workspace>>2"), None);
        assert_eq!(parse_event("not an event"), None);
    }

    #[test]
    fn reads_focus_from_query_replies() {
        let window: Value =
            serde_json::from_str(r#"{"class": "cursor", "title": "x", "monitor": 0}"#)
                .expect("json");
        let monitors: Value = serde_json::from_str(
            r#"[{"id": 0, "name": "DP-1", "focused": false}, {"id": 1, "name": "DP-2", "focused": true}]"#,
        )
        .expect("json");
        assert_eq!(
            focus_from(&window, &monitors),
            Focus {
                app: Some("cursor".to_owned()),
                monitor: Some("DP-2".to_owned())
            }
        );
    }

    #[test]
    fn empty_desktop_has_no_app() {
        let window: Value = serde_json::from_str("{}").expect("json");
        let monitors: Value = serde_json::from_str("[]").expect("json");
        assert_eq!(focus_from(&window, &monitors), Focus::default());
    }
}
