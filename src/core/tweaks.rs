//! Твики Windows: разрешено/запрещено, изменение значений реестра.
//!
//! Данные сгенерированы из Reg Organizer `tweaks.bkp.xml` (см. `tweaks_data.rs`).
//! Перед любым изменением значения сохраняется резервная копия в `.reg` формате
//! в `backups/tweaks/`, а сама операция пишется в журнал операций.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use windows_registry::{CLASSES_ROOT, CURRENT_USER, Key, LOCAL_MACHINE, USERS};

use crate::config;
use crate::core::journal::Journal;

use super::tweaks_data::TWEAKS;

/// Корневой раздел реестра твика.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegRoot {
    LocalMachine,
    CurrentUser,
    ClassesRoot,
    #[allow(dead_code)]
    Users,
}

impl RegRoot {
    fn root(self) -> &'static Key {
        match self {
            Self::LocalMachine => LOCAL_MACHINE,
            Self::CurrentUser => CURRENT_USER,
            Self::ClassesRoot => CLASSES_ROOT,
            Self::Users => USERS,
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::LocalMachine => "HKLM",
            Self::CurrentUser => "HKCU",
            Self::ClassesRoot => "HKCR",
            Self::Users => "HKU",
        }
    }
}

/// Тип значения реестра твика. true — REG_DWORD, false — REG_SZ.
pub type ValueType = bool;

/// Вид твика: фиксированные состояния On/Off или плавающее числовое значение.
#[derive(Debug, Clone, Copy)]
pub enum TweakKind {
    Fixed {
        on_value: &'static str,
        off_value: &'static str,
        default_on: bool,
    },
    Floating {
        default_value: &'static str,
        min: &'static str,
        max: &'static str,
        unit: &'static str,
    },
}

/// Метаданные одного твика.
#[derive(Debug, Clone, Copy)]
pub struct TweakInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub root: RegRoot,
    pub key: &'static str,
    pub value: Option<&'static str>,
    /// REG_DWORD (true) или REG_SZ (false).
    pub value_type: ValueType,
    pub kind: TweakKind,
    pub need_logoff: bool,
    pub need_reboot: bool,
}

/// Все твики в каноническом порядке из сгенерированных данных.
pub fn all() -> &'static [TweakInfo] {
    TWEAKS
}

/// Полный путь к ключу для отображения.
pub fn full_path(info: &TweakInfo) -> String {
    format!("{}\\{}", info.root.short(), info.key)
}

/// Читает текущее значение твика (None — значение отсутствует).
pub fn read_raw(info: &TweakInfo) -> Result<Option<String>> {
    let key = match info.root.root().open(info.key) {
        Ok(key) => key,
        Err(_) => return Ok(None),
    };
    let name = info.value.unwrap_or("");
    let value = if info.value_type {
        key.get_u32(name).map(|v| v.to_string())
    } else {
        key.get_string(name)
    };
    match value {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

/// Для фиксированных твиков: включён ли он сейчас (None — значение не распознано).
pub fn current_enabled(info: &TweakInfo) -> Result<Option<bool>> {
    let TweakKind::Fixed { on_value, off_value, .. } = info.kind else {
        return Ok(None);
    };
    let Some(raw) = read_raw(info)? else {
        return Ok(None);
    };
    if raw == on_value {
        return Ok(Some(true));
    }
    if raw == off_value {
        return Ok(Some(false));
    }
    Ok(None)
}

/// Результат применения твика.
#[derive(Debug)]
pub struct TweakApplyOutcome {
    pub requires_logoff: bool,
    pub requires_reboot: bool,
}

/// Применяет твик: включая/выключая его или устанавливая значение по умолчанию.
pub fn apply(info: &TweakInfo, enable: bool) -> Result<TweakApplyOutcome> {
    let previous = read_raw(info)?;
    backup_reg(info, previous.as_deref())?;

    let target = match info.kind {
        TweakKind::Fixed { on_value, off_value, .. } => {
            if enable {
                on_value
            } else {
                off_value
            }
        }
        TweakKind::Floating { default_value, .. } => default_value,
    };

    write_target(info, target)?;

    record_journal(info, "tweak_applied", previous.as_deref(), Some(target))?;

    Ok(TweakApplyOutcome {
        requires_logoff: info.need_logoff,
        requires_reboot: info.need_reboot,
    })
}

/// Восстанавливает значение твика по умолчанию.
pub fn reset(info: &TweakInfo) -> Result<TweakApplyOutcome> {
    let previous = read_raw(info)?;
    backup_reg(info, previous.as_deref())?;

    let target = match info.kind {
        TweakKind::Fixed {
            on_value,
            off_value,
            default_on,
        } => {
            if default_on {
                on_value
            } else {
                off_value
            }
        }
        TweakKind::Floating { default_value, .. } => default_value,
    };

    write_target(info, target)?;

    record_journal(info, "tweak_reset", previous.as_deref(), Some(target))?;

    Ok(TweakApplyOutcome {
        requires_logoff: info.need_logoff,
        requires_reboot: info.need_reboot,
    })
}

/// Удаляет значение твика (для плавающих — отключение).
#[allow(dead_code)]
pub fn disable_remove(info: &TweakInfo) -> Result<TweakApplyOutcome> {
    let previous = read_raw(info)?;
    backup_reg(info, previous.as_deref())?;

    let key = info
        .root
        .root()
        .open(info.key)
        .context("не удалось открыть ключ реестра")?;
    if let Some(name) = info.value {
        let _ = key.remove_value(name);
    }

    record_journal(info, "tweak_removed", previous.as_deref(), None)?;

    Ok(TweakApplyOutcome {
        requires_logoff: info.need_logoff,
        requires_reboot: info.need_reboot,
    })
}

/// Применяет произвольное значение к плавающему твику (с проверкой диапазона).
pub fn apply_value(info: &TweakInfo, value: &str) -> Result<TweakApplyOutcome> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("значение не может быть пустым");
    }
    validate_value(info, value)?;

    let previous = read_raw(info)?;
    backup_reg(info, previous.as_deref())?;

    write_target(info, value)?;

    record_journal(info, "tweak_value_applied", previous.as_deref(), Some(value))?;

    Ok(TweakApplyOutcome {
        requires_logoff: info.need_logoff,
        requires_reboot: info.need_reboot,
    })
}

fn validate_value(info: &TweakInfo, value: &str) -> Result<()> {
    let TweakKind::Floating { min, max, .. } = info.kind else {
        return Ok(());
    };
    if info.value_type {
        let number = parse_dword(value)
            .with_context(|| format!("неверное DWORD-значение `{value}` для {}", info.title))?;
        if !min.is_empty() {
            let lower = parse_dword(min).unwrap_or(0);
            if number < lower {
                anyhow::bail!("значение {number} меньше допустимого минимума {lower}");
            }
        }
        if !max.is_empty() {
            let upper = parse_dword(max).unwrap_or(u32::MAX);
            if number > upper {
                anyhow::bail!("значение {number} больше допустимого максимума {upper}");
            }
        }
    }
    Ok(())
}

fn write_target(info: &TweakInfo, target: &str) -> Result<()> {
    let key = info
        .root
        .root()
        .create(info.key)
        .with_context(|| format!("не удалось открыть ключ реестра {}", full_path(info)))?;
    let name = info.value.unwrap_or("");

    if info.value_type {
        let number = parse_dword(target).with_context(|| {
            format!("не удалось разобрать DWORD-значение `{target}` для {}", info.title)
        })?;
        key.set_u32(name, number)
            .with_context(|| format!("не удалось записать значение `{target}` для {}", info.title))?;
    } else if target.is_empty() {
        let _ = key.remove_value(name);
    } else {
        key.set_string(name, target)
            .with_context(|| format!("не удалось записать значение `{target}` для {}", info.title))?;
    }
    Ok(())
}

fn parse_dword(value: &str) -> Result<u32> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).context("неверное шестнадцатеричное DWORD-значение");
    }
    value.parse::<u32>().context("неверное DWORD-значение")
}

fn record_journal(
    info: &TweakInfo,
    event: &str,
    previous: Option<&str>,
    applied: Option<&str>,
) -> Result<()> {
    let mut journal = Journal::open()?;
    journal.record(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": event,
        "tweak_id": info.id,
        "tweak_title": info.title,
        "path": full_path(info),
        "value": info.value,
        "previous": previous,
        "applied": applied,
    }))?;
    journal.flush()
}

/// Создаёт резервную копию текущего значения в `.reg` формате.
fn backup_reg(info: &TweakInfo, previous: Option<&str>) -> Result<PathBuf> {
    let dir = config::backups_dir().join("tweaks");
    fs::create_dir_all(&dir)
        .with_context(|| format!("не удалось создать {}", dir.display()))?;

    let id = info.id.trim_matches(|c| c == '{' || c == '}');
    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let path = dir.join(format!("{id}-{stamp}.reg"));

    let root = info.root.reg_name();
    let mut text = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    text.push('[');
    text.push_str(root);
    text.push('\\');
    text.push_str(info.key);
    text.push_str("]\r\n");

    let name = info.value.unwrap_or("");
    if let Some(previous) = previous {
        if info.value_type {
            let number = parse_dword(previous).unwrap_or(0);
            text.push_str(&format!("\"{}\"=dword:{:08x}\r\n", escape_reg_name(name), number));
        } else {
            text.push_str(&format!("\"{}\"=\"{}\"\r\n", escape_reg_name(name), escape_reg_value(previous)));
        }
    } else {
        text.push_str(&format!("\"{}\"=-\r\n", escape_reg_name(name)));
    }

    fs::write(&path, text)
        .with_context(|| format!("не удалось записать резервную копию {}", path.display()))?;
    Ok(path)
}

fn escape_reg_name(name: &str) -> String {
    name.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_reg_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

impl RegRoot {
    fn reg_name(self) -> &'static str {
        match self {
            Self::LocalMachine => "HKEY_LOCAL_MACHINE",
            Self::CurrentUser => "HKEY_CURRENT_USER",
            Self::ClassesRoot => "HKEY_CLASSES_ROOT",
            Self::Users => "HKEY_USERS",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_has_unique_ids() {
        let mut ids: Vec<_> = all().iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all().len(), "дубликаты ID твиков");
    }

    #[test]
    fn data_keys_have_no_root_prefix() {
        for tweak in all() {
            assert!(
                !tweak.key.starts_with("HKEY_"),
                "ключ содержит префикс корня: {}",
                tweak.key
            );
        }
    }

    #[test]
    fn parse_dword_variants() {
        assert_eq!(parse_dword("1").unwrap(), 1);
        assert_eq!(parse_dword("0x00000001").unwrap(), 1);
        assert_eq!(parse_dword("65535").unwrap(), 65535);
        assert!(parse_dword("abc").is_err());
    }
}
