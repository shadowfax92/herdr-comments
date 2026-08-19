use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const DEFAULT_CONFIG: &str = include_str!("../config.default.yaml");
pub const CAPTURE_POPUP: &str = "capture";
pub const REVIEW_POPUP: &str = "review";

#[derive(Debug)]
pub struct LoadedConfig {
    config: Config,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    inline_comments: bool,
    popups: BTreeMap<String, PopupSize>,
    #[serde(default)]
    profiles: Vec<Profile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PopupSize {
    pub width: String,
    pub height: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    name: String,
    #[serde(rename = "match")]
    selector: ProfileMatch,
    #[serde(default)]
    popups: BTreeMap<String, PopupSize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileMatch {
    min_client_width: Option<u16>,
    max_client_width: Option<u16>,
}

impl LoadedConfig {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::write(&path, DEFAULT_CONFIG)
                    .with_context(|| format!("failed to write {}", path.display()))?;
                DEFAULT_CONFIG.to_owned()
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read {}", path.display()))
            }
        };
        let config = Config::parse(&source)
            .with_context(|| format!("invalid comments config {}", path.display()))?;
        Ok(Self { config })
    }

    pub fn popup(&self, name: &str, client_width: Option<u16>) -> Result<PopupSize> {
        self.config.popup(name, client_width)
    }

    pub fn inline_comments(&self) -> bool {
        self.config.inline_comments
    }
}

impl Config {
    fn parse(source: &str) -> Result<Self> {
        let config: Self = noyalib::from_str(source)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        for required in [CAPTURE_POPUP, REVIEW_POPUP] {
            if !self.popups.contains_key(required) {
                bail!("popup `{required}` is required");
            }
        }
        for popup in self.popups.values() {
            validate_popup(popup)?;
        }
        for profile in &self.profiles {
            if profile.name.trim().is_empty() {
                bail!("profile name cannot be empty");
            }
            if profile
                .selector
                .min_client_width
                .zip(profile.selector.max_client_width)
                .is_some_and(|(min, max)| min > max)
            {
                bail!("profile `{}` has min width above max width", profile.name);
            }
            for (name, popup) in &profile.popups {
                if !self.popups.contains_key(name) {
                    bail!(
                        "profile `{}` references unknown popup `{name}`",
                        profile.name
                    );
                }
                validate_popup(popup)?;
            }
        }
        Ok(())
    }

    fn popup(&self, name: &str, client_width: Option<u16>) -> Result<PopupSize> {
        let base = self
            .popups
            .get(name)
            .with_context(|| format!("unknown popup `{name}`"))?;
        let Some(width) = client_width else {
            return Ok(base.clone());
        };
        Ok(self
            .profiles
            .iter()
            .find(|profile| profile.selector.matches(width))
            .and_then(|profile| profile.popups.get(name))
            .unwrap_or(base)
            .clone())
    }
}

impl ProfileMatch {
    fn matches(&self, width: u16) -> bool {
        self.min_client_width.is_none_or(|min| width >= min)
            && self.max_client_width.is_none_or(|max| width <= max)
    }
}

fn validate_popup(popup: &PopupSize) -> Result<()> {
    for (name, value) in [("width", &popup.width), ("height", &popup.height)] {
        let valid = if let Some(percent) = value.strip_suffix('%') {
            percent
                .parse::<u16>()
                .is_ok_and(|number| (1..=100).contains(&number))
        } else {
            value.parse::<u16>().is_ok_and(|number| number > 0)
        };
        if !valid {
            bail!("popup {name} `{value}` must be positive cells or 1%-100%");
        }
    }
    Ok(())
}

fn config_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        return PathBuf::from(dir).join("config.yaml");
    }
    if let Some(path) = std::env::var_os("HERDR_CONFIG_PATH") {
        return PathBuf::from(path)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("plugins/config/shadowfax.comments/config.yaml");
    }
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("herdr/plugins/config/shadowfax.comments/config.yaml");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config/herdr/plugins/config/shadowfax.comments/config.yaml");
    }
    std::env::temp_dir().join("herdr/plugins/config/shadowfax.comments/config.yaml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config::parse(DEFAULT_CONFIG).unwrap()
    }

    #[test]
    fn inline_comments_are_enabled_by_default_and_legacy_configs_stay_disabled() {
        assert!(config().inline_comments);

        let legacy = DEFAULT_CONFIG.replacen("inline_comments: true\n\n", "", 1);
        assert!(!Config::parse(&legacy).unwrap().inline_comments);

        let disabled =
            DEFAULT_CONFIG.replacen("inline_comments: true", "inline_comments: false", 1);
        assert!(!Config::parse(&disabled).unwrap().inline_comments);
    }

    #[test]
    fn selects_responsive_popup_sizes() {
        let config = config();

        assert_eq!(config.popup(CAPTURE_POPUP, Some(300)).unwrap().width, "95%");
        assert_eq!(config.popup(CAPTURE_POPUP, Some(330)).unwrap().width, "90%");
        assert_eq!(config.popup(CAPTURE_POPUP, Some(380)).unwrap().width, "90%");
        assert_eq!(config.popup(CAPTURE_POPUP, Some(512)).unwrap().width, "50%");
        assert_eq!(
            config.popup(CAPTURE_POPUP, Some(512)).unwrap().height,
            "70%"
        );
        assert_eq!(config.popup(REVIEW_POPUP, Some(512)).unwrap().width, "70%");
        assert_eq!(config.popup(REVIEW_POPUP, Some(512)).unwrap().height, "85%");
    }

    #[test]
    fn rejects_invalid_popup_sizes() {
        let source = DEFAULT_CONFIG.replace("height: \"90%\"", "height: \"120%\"");

        assert!(Config::parse(&source).is_err());
    }
}
