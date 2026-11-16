//! Deliberately small, self-contained GUI language preference.
//! Backend and journal messages are diagnostic output and remain verbatim.

use anyhow::Result;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Polish,
    English,
}

static ACTIVE: AtomicU8 = AtomicU8::new(0);

impl Language {
    pub fn selected() -> Self {
        if ACTIVE.load(Ordering::Relaxed) == 1 {
            Self::English
        } else {
            Self::Polish
        }
    }

    pub fn index(self) -> u32 {
        match self {
            Self::Polish => 0,
            Self::English => 1,
        }
    }

    pub fn from_index(value: u32) -> Self {
        if value == 1 {
            Self::English
        } else {
            Self::Polish
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
            Self::Polish => "pl\n",
            Self::English => "en\n",
        };
        crate::backend::atomic_write(&config_path(), value.as_bytes())?;
        ACTIVE.store(self.index() as u8, Ordering::Relaxed);
        Ok(())
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "pl" => Some(Self::Polish),
            "en" => Some(Self::English),
            _ => None,
        }
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
    match Language::selected() {
        Language::Polish => pl,
        Language::English => en,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_file_accepts_only_known_codes() {
        assert_eq!(Language::parse("pl\n"), Some(Language::Polish));
        assert_eq!(Language::parse("en\n"), Some(Language::English));
        assert_eq!(Language::parse("fr"), None);
    }

    #[test]
    fn language_index_round_trip() {
        for language in [Language::Polish, Language::English] {
            assert_eq!(Language::from_index(language.index()), language);
        }
    }
}
