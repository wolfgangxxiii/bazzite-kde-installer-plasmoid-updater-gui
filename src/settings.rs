use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub automatic: bool,
    pub auto_update_tool: bool,
    pub frequency_hours: u8,
    pub restart_plasma: bool,
    pub notify_on_success: bool,
    pub notify_on_error: bool,
    pub excluded_packages: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            automatic: true,
            auto_update_tool: true,
            frequency_hours: 24,
            restart_plasma: false,
            notify_on_success: false,
            notify_on_error: true,
            excluded_packages: Vec::new(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            matches!(self.frequency_hours, 6 | 12 | 24),
            "Częstotliwość musi wynosić 6, 12 albo 24 godziny"
        );
        Ok(())
    }

    pub fn parse_exclusions(input: &str) -> Vec<String> {
        let mut names = Vec::new();
        for raw in input.split(',') {
            let name = raw.trim();
            if !name.is_empty() && !names.iter().any(|existing| existing == name) {
                names.push(name.to_owned());
            }
        }
        names
    }

    pub fn load(home: &Path) -> Result<Self> {
        let path = Self::path(home);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("Nie można odczytać {}", path.display()))?;
        let settings: Self =
            toml::from_str(&text).with_context(|| format!("Błędny plik {}", path.display()))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, home: &Path) -> Result<()> {
        self.validate()?;
        let path = Self::path(home);
        super::backend::atomic_write(&path, toml::to_string_pretty(self)?.as_bytes())
    }

    pub fn path(home: &Path) -> PathBuf {
        home.join(".config/bazzite-plasmoid-updater-gui/settings.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_trim_and_deduplicate() {
        assert_eq!(
            Settings::parse_exclusions("weather, clock, weather, , cursor"),
            vec!["weather", "clock", "cursor"]
        );
    }

    #[test]
    fn defaults_are_unattended_but_do_not_restart_desktop() {
        let settings = Settings::default();
        assert!(settings.automatic);
        assert!(settings.auto_update_tool);
        assert!(!settings.restart_plasma);
        assert!(!settings.notify_on_success);
        assert!(settings.notify_on_error);
        assert_eq!(settings.frequency_hours, 24);
    }

    #[test]
    fn rejects_unknown_interval() {
        let mut settings = Settings::default();
        settings.frequency_hours = 5;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn older_settings_receive_safe_notification_defaults() {
        let previous = "automatic = true\nauto_update_tool = true\nfrequency_hours = 24\nrestart_plasma = false\nexcluded_packages = []\n";
        let loaded: Settings = toml::from_str(previous).unwrap();
        assert!(!loaded.notify_on_success);
        assert!(loaded.notify_on_error);
    }
}
