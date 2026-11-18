//! Deliberately small, self-contained GUI language preference.
//! Backend and journal messages are diagnostic output and remain verbatim.

use anyhow::Result;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    System,
    Polish,
    English,
}

// Keep the previous default (Polish) for users without a language setting.
static ACTIVE: AtomicU8 = AtomicU8::new(1);

impl Language {
    pub fn selected() -> Self {
        match ACTIVE.load(Ordering::Relaxed) {
            0 => Self::System,
            2 => Self::English,
            _ => Self::Polish,
        }
    }

    pub fn effective() -> Self {
        match Self::selected() {
            Self::System => Self::system_language(),
            choice => choice,
        }
    }

    fn system_language() -> Self {
        for key in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
            if let Ok(locale) = env::var(key) {
                if let Some(language) = locale_language(&locale) {
                    return language;
                }
            }
        }
        Self::English
    }

    pub fn index(self) -> u32 {
        match self {
            Self::System => 0,
            Self::Polish => 1,
            Self::English => 2,
        }
    }

    pub fn from_index(value: u32) -> Self {
        match value {
            0 => Self::System,
            2 => Self::English,
            _ => Self::Polish,
        }
    }

    pub fn initialize() {
        let value = fs::read_to_string(config_path())
            .ok()
            .and_then(|value| Self::parse(&value))
            .unwrap_or(Self::Polish);
        ACTIVE.store(value.index() as u8, Ordering::Relaxed);
    }

    pub fn save(self) -> Result<()> {
        let value = match self {
            Self::System => "auto\n",
            Self::Polish => "pl\n",
            Self::English => "en\n",
        };
        crate::backend::atomic_write(&config_path(), value.as_bytes())?;
        ACTIVE.store(self.index() as u8, Ordering::Relaxed);
        Ok(())
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "auto" => Some(Self::System),
            "pl" => Some(Self::Polish),
            "en" => Some(Self::English),
            _ => None,
        }
    }
}

fn locale_language(locale: &str) -> Option<Language> {
    let locale = locale.split(':').next()?.trim().to_ascii_lowercase();
    if locale.is_empty() || matches!(locale.as_str(), "c" | "posix") {
        return None;
    }
    if locale == "pl" || locale.starts_with("pl_") || locale.starts_with("pl-") {
        Some(Language::Polish)
    } else {
        Some(Language::English)
    }
}

fn config_path() -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("bazzite-plasmoid-updater-gui/language")
}

pub fn t(pl: &'static str, en: &'static str) -> &'static str {
    match Language::effective() {
        Language::Polish => pl,
        Language::English | Language::System => en,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_file_accepts_only_known_codes() {
        assert_eq!(Language::parse("auto\n"), Some(Language::System));
        assert_eq!(Language::parse("pl\n"), Some(Language::Polish));
        assert_eq!(Language::parse("en\n"), Some(Language::English));
        assert_eq!(Language::parse("fr"), None);
    }

    #[test]
    fn language_index_round_trip() {
        for language in [Language::System, Language::Polish, Language::English] {
            assert_eq!(Language::from_index(language.index()), language);
        }
    }

    #[test]
    fn detects_supported_system_language_with_safe_fallback() {
        assert_eq!(locale_language("pl_PL.UTF-8"), Some(Language::Polish));
        assert_eq!(locale_language("pl:en_US"), Some(Language::Polish));
        assert_eq!(locale_language("de_DE.UTF-8"), Some(Language::English));
        assert_eq!(locale_language("C"), None);
    }
}
