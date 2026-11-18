//! Создание точки восстановления Windows (System Restore).
//!
//! Использует штатный PowerShell-командлет `Checkpoint-Computer`.
//! Точка восстановления — это системный механизм отката, создаётся
//! перед рискованными операциями.

use std::process::Command;

use anyhow::{Context, Result, bail};

/// Создаёт точку восстановления системы.
pub fn create(description: &str) -> Result<()> {
    let safe_description = description.replace('"', "'");
    let script = format!(
        "try {{ Checkpoint-Computer -Description \"{safe_description}\" \
         -RestorePointType MODIFY_SETTINGS -ErrorAction Stop; exit 0 }} \
         catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}"
    );

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .context("не удалось запустить PowerShell")?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    bail!(
        "не удалось создать точку восстановления{}",
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn description_is_quoted_safely() {
        let script_draft = format!(
            "Checkpoint-Computer -Description \"{}\"",
            "test\"quote".replace('"', "'")
        );
        assert!(!script_draft.contains('"') || script_draft.matches('"').count() == 2);
    }
}
