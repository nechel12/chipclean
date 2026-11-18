//! Интеграция с Проводником Windows: контекстное меню для `.reg` файлов.
//!
//! Регистрирует команду «Предпросмотр в ChipClean» для `.reg` файлов.
//! При выборе пункта Проводник запускает `chipclean.exe --preview-reg <файл>`,
//! а приложение открывает предпросмотр импорта разделов.

use anyhow::{Context, Result};
use windows_registry::{CLASSES_ROOT, Key};

const REG_SHELL_KEY: &str = r".reg\shell\ChipCleanPreview";
const REG_COMMAND_KEY: &str = r".reg\shell\ChipCleanPreview\command";
const MENU_TITLE: &str = "Предпросмотр в ChipClean";

/// Регистрирует пункт контекстного меню для `.reg` файлов.
pub fn register_reg_preview() -> Result<()> {
    let exe = std::env::current_exe().context("не удалось получить путь к исполняемому файлу")?;
    let command = format!("\"{}\" --preview-reg \"%1\"", exe.display());
    let exe_display = exe.display().to_string();

    let shell = CLASSES_ROOT.create(REG_SHELL_KEY)?;
    shell.set_string("", MENU_TITLE)?;
    shell.set_string("Icon", &exe_display)?;
    shell.set_string("MUIVerb", MENU_TITLE)?;

    let command_key = CLASSES_ROOT.create(REG_COMMAND_KEY)?;
    command_key.set_string("", &command)?;

    Ok(())
}

/// Удаляет пункт контекстного меню для `.reg` файлов.
pub fn unregister_reg_preview() -> Result<()> {
    CLASSES_ROOT.remove_tree(REG_SHELL_KEY)?;
    Ok(())
}

/// Проверяет, зарегистрировано ли контекстное меню.
pub fn is_reg_preview_registered() -> bool {
    Key::open(CLASSES_ROOT, REG_COMMAND_KEY).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_key_path_is_stable() {
        assert_eq!(REG_SHELL_KEY, r".reg\shell\ChipCleanPreview");
        assert_eq!(REG_COMMAND_KEY, r".reg\shell\ChipCleanPreview\command");
    }
}
