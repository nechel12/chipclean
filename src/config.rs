use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub max_cleanup: bool,
    pub downloads_older_than_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_cleanup: false,
            downloads_older_than_days: 7,
        }
    }
}

impl Settings {
    fn normalized(mut self) -> Self {
        self.downloads_older_than_days = self.downloads_older_than_days.clamp(1, 365);
        self
    }
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("ru", "ChipClean", "ChipClean")
}

pub fn config_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn log_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("ChipClean"))
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn journal_path() -> PathBuf {
    log_dir().join("operations.jsonl")
}

pub fn backups_dir() -> PathBuf {
    log_dir().join("backups")
}

pub fn startup_store_path() -> PathBuf {
    log_dir().join("disabled-startup.json")
}

pub fn startup_quarantine_dir() -> PathBuf {
    log_dir().join("startup-disabled")
}

pub fn traces_store_path() -> PathBuf {
    log_dir().join("traces-quarantine.json")
}

pub fn traces_quarantine_dir() -> PathBuf {
    log_dir().join("traces-quarantine")
}

pub fn load() -> Settings {
    let path = settings_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return Settings::default();
    };

    serde_json::from_str::<Settings>(raw.trim_start_matches('\u{feff}'))
        .map(Settings::normalized)
        .unwrap_or_else(|err| {
            tracing::warn!(%err, path = %path.display(), "failed to parse settings");
            Settings::default()
        })
}

pub fn save(settings: &Settings) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("не удалось создать каталог {}", dir.display()))?;
    let path = settings_path();
    let raw = serde_json::to_string_pretty(settings)?;
    fs::write(&path, raw).with_context(|| format!("не удалось записать {}", path.display()))?;
    Ok(())
}

pub fn open_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).ok();
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .context("не удалось открыть проводник")?;
    Ok(())
}
