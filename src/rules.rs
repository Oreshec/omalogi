//! Automatic profile rules, read from `$XDG_CONFIG_HOME/omalogi/config.toml`.
//!
//! ```toml
//! # Profile to use when no rule matches. Leave it out to keep the current profile.
//! default_profile = 1
//!
//! # Rules are checked in order; the first match wins.
//! [[rule]]
//! app = "cs2"        # Hyprland window class
//! profile = 2
//!
//! [[rule]]
//! monitor = "DP-2"   # Hyprland monitor name
//! profile = 1
//! ```

use std::{
    io,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read {path}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("{path} is not valid: {message}")]
    Invalid { path: String, message: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Profile number (1-based) used when no rule matches.
    pub default_profile: Option<usize>,
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Window class to match, compared without regard to case.
    pub app: Option<String>,
    /// Monitor name to match, e.g. `DP-2`.
    pub monitor: Option<String>,
    /// Profile number (1-based) to activate.
    pub profile: usize,
}

/// What currently has the user's attention.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    pub app: Option<String>,
    pub monitor: Option<String>,
}

/// Why a profile was chosen, for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The rule at this position (0-based) matched.
    Rule(usize),
    Default,
}

impl Config {
    /// `$XDG_CONFIG_HOME/omalogi/config.toml`, falling back to `~/.config`.
    #[must_use]
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::home_dir().map(|home| home.join(".config")))?;
        Some(base.join("omalogi/config.toml"))
    }

    /// Loads the config; a missing file is an empty config.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|message| ConfigError::Invalid {
                path: path.display().to_string(),
                message,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.display().to_string(),
                source,
            }),
        }
    }

    /// Parses and validates config text. Errors are human-readable.
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|error| match error.span() {
            Some(span) => {
                let line = text[..span.start].matches('\n').count() + 1;
                format!("line {line}: {}", error.message())
            }
            None => error.message().to_owned(),
        })?;
        if config.default_profile == Some(0) {
            return Err("default_profile must be 1 or higher".to_owned());
        }
        for (index, rule) in config.rules.iter().enumerate() {
            let number = index + 1;
            if rule.profile == 0 {
                return Err(format!("rule {number}: profile must be 1 or higher"));
            }
            if rule.app.is_none() && rule.monitor.is_none() {
                return Err(format!("rule {number} needs an app, a monitor, or both"));
            }
        }
        Ok(config)
    }

    /// The profile for `focus`: the first rule whose given fields all match, else the default.
    #[must_use]
    pub fn profile_for(&self, focus: &Focus) -> Option<(usize, Reason)> {
        self.rules
            .iter()
            .position(|rule| rule.matches(focus))
            .map(|index| (self.rules[index].profile, Reason::Rule(index)))
            .or_else(|| {
                self.default_profile
                    .map(|profile| (profile, Reason::Default))
            })
    }
}

impl Rule {
    fn matches(&self, focus: &Focus) -> bool {
        let app = self.app.as_ref().is_none_or(|want| {
            focus
                .app
                .as_ref()
                .is_some_and(|have| have.eq_ignore_ascii_case(want))
        });
        let monitor = self
            .monitor
            .as_ref()
            .is_none_or(|want| focus.monitor.as_ref() == Some(want));
        app && monitor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus(app: Option<&str>, monitor: Option<&str>) -> Focus {
        Focus {
            app: app.map(str::to_owned),
            monitor: monitor.map(str::to_owned),
        }
    }

    const EXAMPLE: &str = r#"
        default_profile = 1

        [[rule]]
        app = "cs2"
        monitor = "DP-1"
        profile = 3

        [[rule]]
        app = "cs2"
        profile = 2

        [[rule]]
        monitor = "HDMI-A-1"
        profile = 4
    "#;

    #[test]
    fn first_matching_rule_wins() {
        let config = Config::parse(EXAMPLE).expect("example parses");
        assert_eq!(
            config.profile_for(&focus(Some("cs2"), Some("DP-1"))),
            Some((3, Reason::Rule(0)))
        );
        assert_eq!(
            config.profile_for(&focus(Some("cs2"), Some("DP-2"))),
            Some((2, Reason::Rule(1)))
        );
    }

    #[test]
    fn app_matching_ignores_case() {
        let config = Config::parse(EXAMPLE).expect("example parses");
        assert_eq!(
            config.profile_for(&focus(Some("CS2"), None)),
            Some((2, Reason::Rule(1)))
        );
    }

    #[test]
    fn monitor_rules_apply_to_any_app() {
        let config = Config::parse(EXAMPLE).expect("example parses");
        assert_eq!(
            config.profile_for(&focus(Some("firefox"), Some("HDMI-A-1"))),
            Some((4, Reason::Rule(2)))
        );
    }

    #[test]
    fn falls_back_to_default_or_nothing() {
        let config = Config::parse(EXAMPLE).expect("example parses");
        assert_eq!(
            config.profile_for(&focus(Some("firefox"), Some("DP-2"))),
            Some((1, Reason::Default))
        );
        assert_eq!(
            Config::default().profile_for(&focus(Some("cs2"), None)),
            None
        );
    }

    #[test]
    fn app_rules_need_a_focused_app() {
        let config = Config::parse("[[rule]]\napp = \"cs2\"\nprofile = 2\n").expect("parses");
        assert_eq!(config.profile_for(&focus(None, Some("DP-2"))), None);
    }

    #[test]
    fn rejects_invalid_configs_with_reasons() {
        assert_eq!(
            Config::parse("[[rule]]\nprofile = 2\n"),
            Err("rule 1 needs an app, a monitor, or both".to_owned())
        );
        assert_eq!(
            Config::parse("[[rule]]\napp = \"x\"\nprofile = 0\n"),
            Err("rule 1: profile must be 1 or higher".to_owned())
        );
        assert_eq!(
            Config::parse("default_profile = 0\n"),
            Err("default_profile must be 1 or higher".to_owned())
        );
        let unknown = Config::parse("[[rule]]\napplication = \"x\"\nprofile = 1\n")
            .expect_err("unknown key is refused");
        assert!(unknown.contains("application"), "{unknown}");
        let broken =
            Config::parse("default_profile = 1\n\n[[rule]]\napp = cs2\n").expect_err("broken");
        assert!(broken.starts_with("line 4: "), "{broken}");
    }

    #[test]
    fn missing_file_is_an_empty_config() {
        let path = std::env::temp_dir().join("omalogi-no-such-config.toml");
        assert_eq!(Config::load(&path).expect("loads"), Config::default());
    }
}
