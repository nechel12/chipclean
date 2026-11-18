//! Состояние страницы «Твики»: список, текущие значения, ожидающие изменения.

use anyhow::Result;

use super::tweaks::{self, TweakInfo};

#[derive(Debug, Clone)]
pub struct TweakEntry {
    pub info: &'static TweakInfo,
    /// Для фиксированных: включён ли сейчас (None — значение не распознано).
    pub current: Option<bool>,
    /// Текущее значение в виде строки (для отображения).
    pub current_raw: Option<String>,
    /// Ожидающее изменение (None — нет изменения).
    pub pending: Option<bool>,
}

#[derive(Debug, Default)]
pub struct TweaksState {
    pub entries: Vec<TweakEntry>,
    pub scanned: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub applied_summary: Option<String>,
    pub editing: Option<usize>,
    pub edit_buffer: String,
}

impl TweaksState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn scan(&mut self) {
        self.busy = true;
        self.error = None;
        self.applied_summary = None;

        let mut entries = Vec::with_capacity(tweaks::all().len());
        for info in tweaks::all() {
            let (current, current_raw) = match tweaks::read_raw(info) {
                Ok(raw) => {
                    let enabled = tweaks::current_enabled(info).unwrap_or(None);
                    (enabled, raw)
                }
                Err(err) => {
                    self.error = Some(format!("Не удалось прочитать «{}»: {err}", info.title));
                    (None, None)
                }
            };
            entries.push(TweakEntry {
                info,
                current,
                current_raw,
                pending: None,
            });
        }

        self.entries = entries;
        self.scanned = true;
        self.busy = false;
    }

    pub fn toggle(&mut self, index: usize, enable: bool) {
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        match entry.pending {
            Some(pending) if pending == enable => entry.pending = None,
            _ => entry.pending = Some(enable),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.pending.is_some()).count()
    }

    pub fn discard_changes(&mut self) {
        for entry in &mut self.entries {
            entry.pending = None;
        }
        self.applied_summary = None;
        self.editing = None;
    }

    /// Начинает редактирование значения плавающего твика.
    pub fn begin_edit(&mut self, index: usize) {
        if self.entries.get(index).is_some() {
            self.editing = Some(index);
            self.edit_buffer = self.entries[index]
                .current_raw
                .clone()
                .unwrap_or_default();
        }
    }

    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Применяет введённое значение плавающего твика.
    pub fn apply_edit(&mut self, index: usize) -> Result<()> {
        let Some(entry) = self.entries.get(index) else {
            self.editing = None;
            return Ok(());
        };
        let value = self.edit_buffer.trim().to_owned();
        if value.is_empty() {
            self.editing = None;
            return Ok(());
        }
        tweaks::apply_value(entry.info, &value)?;
        self.editing = None;
        self.scan();
        Ok(())
    }

    /// Применяет все ожидающие изменения. Возвращает сводку.
    pub fn apply_pending(&mut self) -> Result<String> {
        let pending: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| entry.pending.map(|enable| (entry.info, enable)))
            .collect();

        let mut applied = 0usize;
        let mut need_logoff = false;
        let mut need_reboot = false;

        for (info, enable) in pending {
            match tweaks::apply(info, enable) {
                Ok(outcome) => {
                    applied += 1;
                    need_logoff |= outcome.requires_logoff;
                    need_reboot |= outcome.requires_reboot;
                }
                Err(err) => {
                    self.error = Some(format!("Не удалось применить «{}»: {err}", info.title));
                    continue;
                }
            }
        }

        self.scan();
        self.discard_changes();

        let mut summary = format!("Применено изменений: {applied}.");
        if need_logoff {
            summary.push_str(" Для вступления в силу нужен выход из Windows (Logoff).");
        }
        if need_reboot {
            summary.push_str(" Для вступления в силу нужна перезагрузка компьютера.");
        }
        self.applied_summary = Some(summary.clone());
        Ok(summary)
    }

    /// Восстанавливает один твик к значению по умолчанию.
    pub fn reset_one(&mut self, index: usize) -> Result<()> {
        let Some(entry) = self.entries.get(index) else {
            return Ok(());
        };
        tweaks::reset(entry.info)?;
        self.scan();
        Ok(())
    }
}
