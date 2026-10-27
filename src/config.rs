use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::core::fs_atomic;

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

pub fn snapshots_dir() -> PathBuf {
    log_dir().join("snapshots")
}

pub fn community_db_path() -> PathBuf {
    log_dir().join("winapp2.ini")
}

pub fn startup_store_path() -> PathBuf {
    log_dir().join("disabled-startup.json")
}

pub fn startup_quarantine_dir() -> PathBuf {
    log_dir().join("startup-disabled")
}

pub fn disabled_services_path() -> PathBuf {
    log_dir().join("disabled-services.json")
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
    fs_atomic::write_json_atomic(&settings_path(), settings)
}

pub fn open_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).ok();
    Command::new(explorer_exe())
        .arg(path)
        .spawn()
        .context("не удалось открыть проводник")?;
    Ok(())
}

fn explorer_exe() -> PathBuf {
    std::env::var_os("WINDIR")
        .or_else(|| std::env::var_os("SystemRoot"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("explorer.exe")
}
