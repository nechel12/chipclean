//! Фоновая очистка по расписанию через Планировщик заданий Windows.
//!
//! Регистрируется одна ежедневная задача `ChipClean Background Cleanup`,
//! которая запускает текущий исполняемый файл с аргументом
//! `--background-cleanup` (см. `main.rs`).

use std::process::Command;

use anyhow::{Context, Result};

pub const TASK_NAME: &str = "ChipClean Background Cleanup";
pub const TASK_STARTUP_NAME: &str = "ChipClean Startup Check";

/// Проверяет, зарегистрирована ли задача.
pub fn is_registered() -> bool {
    task_exists(TASK_NAME)
}

/// Проверяет, зарегистрирована ли задача проверки при входе.
pub fn is_startup_check_registered() -> bool {
    task_exists(TASK_STARTUP_NAME)
}

fn task_exists(name: &str) -> bool {
    let output = Command::new("schtasks")
        .args(["/Query", "/TN", name])
        .output();
    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Создаёт или обновляет ежедневную задачу очистки.
pub fn register(hour: u32, minute: u32) -> Result<()> {
    let exe = std::env::current_exe().context("не удалось получить путь к исполняемому файлу")?;
    let hour = hour.min(23);
    let minute = minute.min(59);
    let start = format!("{hour:02}:{minute:02}");
    let command = format!("\"{}\" --background-cleanup", exe.display());

    let status = Command::new("schtasks")
        .args([
            "/Create",
            "/TN",
            TASK_NAME,
            "/TR",
            &command,
            "/SC",
            "DAILY",
            "/ST",
            &start,
            "/RL",
            "HIGHEST",
            "/F",
        ])
        .status()
        .context("не удалось запустить schtasks")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Планировщик не смог создать задачу (код {status})");
    }
}

/// Удаляет задачу очистки.
pub fn unregister() -> Result<()> {
    let status = Command::new("schtasks")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .status()
        .context("не удалось запустить schtasks")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Планировщик не смог удалить задачу (код {status})");
    }
}

/// Регистрирует задачу проверки при входе в систему.
pub fn register_startup_check() -> Result<()> {
    let exe = std::env::current_exe().context("не удалось получить путь к исполняемому файлу")?;
    let command = format!("\"{}\" --background-cleanup", exe.display());

    let status = Command::new("schtasks")
        .args([
            "/Create",
            "/TN",
            TASK_STARTUP_NAME,
            "/TR",
            &command,
            "/SC",
            "ONLOGON",
            "/RL",
            "HIGHEST",
            "/F",
        ])
        .status()
        .context("не удалось запустить schtasks")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Планировщик не смог создать задачу входа (код {status})");
    }
}

/// Удаляет задачу проверки при входе.
pub fn unregister_startup_check() -> Result<()> {
    let status = Command::new("schtasks")
        .args(["/Delete", "/TN", TASK_STARTUP_NAME, "/F"])
        .status()
        .context("не удалось запустить schtasks")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Планировщик не смог удалить задачу входа (код {status})");
    }
}

/// Синхронизирует задачу проверки при входе с настройками.
pub fn apply_startup_check_enabled(enabled: bool) -> Result<()> {
    if enabled {
        register_startup_check()
    } else {
        unregister_startup_check()
    }
}

/// Синхронизирует задачу с настройками.
pub fn apply_enabled(enabled: bool, hour: u32, minute: u32) -> Result<()> {
    if enabled {
        register(hour, minute)
    } else {
        unregister()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_name_is_stable() {
        assert_eq!(TASK_NAME, "ChipClean Background Cleanup");
    }
}
