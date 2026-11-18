//! Твики Windows: разрешено/запрещено, изменение значений реестра.
//!
//! Данные сгенерированы из Reg Organizer `tweaks.bkp.xml` (см. `tweaks_data.rs`).
//! Значения типизированы (Dword/Text/Absent) — разбор строк на лету не нужен.
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

    fn reg_name(self) -> &'static str {
        match self {
            Self::LocalMachine => "HKEY_LOCAL_MACHINE",
            Self::CurrentUser => "HKEY_CURRENT_USER",
            Self::ClassesRoot => "HKEY_CLASSES_ROOT",
            Self::Users => "HKEY_USERS",
        }
    }
}

/// Тип значения реестра твика.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Dword,
    String,
}

/// Типизированное значение твика.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TweakValue {
    Dword(u32),
    Text(&'static str),
    Absent,
}

/// Значение, применяемое к реестру (может быть введено пользователем).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedValue {
    Dword(u32),
    Text(String),
    Absent,
}

impl AppliedValue {
    pub fn display(&self) -> String {
        match self {
            Self::Dword(number) => number.to_string(),
            Self::Text(text) => text.clone(),
            Self::Absent => String::new(),
        }
    }
}

fn to_applied(value: TweakValue) -> AppliedValue {
    match value {
        TweakValue::Dword(number) => AppliedValue::Dword(number),
        TweakValue::Text(text) => AppliedValue::Text(text.to_owned()),
        TweakValue::Absent => AppliedValue::Absent,
    }
}

/// Вид твика: фиксированные состояния On/Off или плавающее значение.
#[derive(Debug, Clone, Copy)]
pub enum TweakKind {
    Fixed {
        on_value: TweakValue,
        off_value: TweakValue,
        default_on: bool,
    },
    Floating {
        default_value: TweakValue,
        min: Option<u32>,
        max: Option<u32>,
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
    let value = if info.value_type == ValueType::Dword {
        key.get_u32(name).map(|number| number.to_string())
    } else {
        key.get_string(name)
    };
    match value {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

fn value_matches(raw: &str, expected: &TweakValue) -> bool {
    match expected {
        TweakValue::Dword(number) => raw == number.to_string().as_str(),
        TweakValue::Text(text) => raw == *text,
        TweakValue::Absent => raw.is_empty(),
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
    if value_matches(&raw, &on_value) {
        return Ok(Some(true));
    }
    if value_matches(&raw, &off_value) {
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

/// Применяет твик: включая/выключая его.
pub fn apply(info: &TweakInfo, enable: bool) -> Result<TweakApplyOutcome> {
    let TweakKind::Fixed { on_value, off_value, .. } = info.kind else {
        return Ok(TweakApplyOutcome {
            requires_logoff: info.need_logoff,
            requires_reboot: info.need_reboot,
        });
    };

    let previous = read_raw(info)?;
    backup_reg(info, previous.as_deref())?;

    let target = if enable { on_value } else { off_value };
    let target = to_applied(target);
    write_target(info, &target)?;

    record_journal(info, "tweak_applied", previous.as_deref(), Some(&target))?;

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
    let target = to_applied(target);
    write_target(info, &target)?;

    record_journal(info, "tweak_reset", previous.as_deref(), Some(&target))?;

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

    let target = match info.value_type {
        ValueType::Dword => {
            let number = parse_dword(value)
                .with_context(|| format!("неверное DWORD-значение `{value}` для {}", info.title))?;
            if let TweakKind::Floating { min, max, .. } = info.kind {
                if let Some(lower) = min
                    && number < lower
                {
                    anyhow::bail!("значение {number} меньше допустимого минимума {lower}");
                }
                if let Some(upper) = max
                    && number > upper
                {
                    anyhow::bail!("значение {number} больше допустимого максимума {upper}");
                }
            }
            AppliedValue::Dword(number)
        }
        ValueType::String => AppliedValue::Text(value.to_owned()),
    };

    let previous = read_raw(info)?;
    backup_reg(info, previous.as_deref())?;
    write_target(info, &target)?;
    record_journal(info, "tweak_value_applied", previous.as_deref(), Some(&target))?;

    Ok(TweakApplyOutcome {
        requires_logoff: info.need_logoff,
        requires_reboot: info.need_reboot,
    })
}

fn write_target(info: &TweakInfo, target: &AppliedValue) -> Result<()> {
    let key = info
        .root
        .root()
        .create(info.key)
        .with_context(|| format!("не удалось открыть ключ реестра {}", full_path(info)))?;
    let name = info.value.unwrap_or("");

    match target {
        AppliedValue::Dword(number) => {
            key.set_u32(name, *number).with_context(|| {
                format!("не удалось записать значение `{number}` для {}", info.title)
            })?;
        }
        AppliedValue::Text(text) => {
            if text.is_empty() {
                let _ = key.remove_value(name);
            } else {
                key.set_string(name, text).with_context(|| {
                    format!("не удалось записать значение `{text}` для {}", info.title)
                })?;
            }
        }
        AppliedValue::Absent => {
            let _ = key.remove_value(name);
        }
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
    applied: Option<&AppliedValue>,
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
        "applied": applied.map(AppliedValue::display),
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

    let mut text = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    text.push('[');
    text.push_str(info.root.reg_name());
    text.push('\\');
    text.push_str(info.key);
    text.push_str("]\r\n");

    let name = info.value.unwrap_or("");
    if let Some(previous) = previous {
        if info.value_type == ValueType::Dword {
            let number = parse_dword(previous).unwrap_or(0);
            text.push_str(&format!("\"{}\"=dword:{number:08x}\r\n", escape_reg_name(name)));
        } else {
            text.push_str(&format!(
                "\"{}\"=\"{}\"\r\n",
                escape_reg_name(name),
                escape_reg_value(previous)
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_has_unique_ids() {
        let mut ids: Vec<_> = all().iter().map(|tweak| tweak.id).collect();
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
    fn values_are_consistent_with_types() {
        for tweak in all() {
            let values: [TweakValue; 2] = match tweak.kind {
                TweakKind::Fixed { on_value, off_value, .. } => [on_value, off_value],
                TweakKind::Floating { default_value, .. } => [default_value, TweakValue::Absent],
            };
            for value in values {
                match (tweak.value_type, value) {
                    (ValueType::Dword, TweakValue::Dword(_)) => {}
                    (ValueType::String, TweakValue::Text(_)) => {}
                    (ValueType::String, TweakValue::Absent) => {}
                    (ValueType::Dword, TweakValue::Absent) => {}
                    other => panic!("несовместимые тип/значение: {other:?}"),
                }
            }
        }
    }

    #[test]
    fn parse_dword_variants() {
        assert_eq!(parse_dword("1").unwrap(), 1);
        assert_eq!(parse_dword("0x00000001").unwrap(), 1);
        assert_eq!(parse_dword("65535").unwrap(), 65535);
        assert!(parse_dword("abc").is_err());
    }

    #[test]
    fn value_matches_compares_canonical_forms() {
        assert!(value_matches("1", &TweakValue::Dword(1)));
        assert!(!value_matches("2", &TweakValue::Dword(1)));
        assert!(value_matches("hello", &TweakValue::Text("hello")));
        assert!(value_matches("", &TweakValue::Absent));
    }
}
