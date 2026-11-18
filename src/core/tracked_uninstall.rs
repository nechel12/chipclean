//! Мастер деинсталляции со снимком реестра.
//!
//! Перед запуском штатного деинсталлятора программа сохраняет список ключей
//! в `HKCU\Software` и `HKLM\Software` (2 уровня). После завершения удаления
//! список сканируется снова — исчезнувшие ключи предлагаются к очистке.

use anyhow::{Context, Result};
use windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE};

/// Запись снимка: куст + относительный путь к ключу.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistrySnapshotEntry {
    pub hive: &'static str,
    pub path: String,
}

impl RegistrySnapshotEntry {
    pub fn display(&self) -> String {
        format!("{}\\{}", self.hive, self.path)
    }
}

/// Снимает список ключей (2 уровня глубины) в Software-разделах.
pub fn capture_registry_snapshot() -> Vec<RegistrySnapshotEntry> {
    let mut out = Vec::new();
    collect(CURRENT_USER, "HKCU", r"Software", 1, &mut out);
    collect(LOCAL_MACHINE, "HKLM", r"Software", 1, &mut out);
    out
}

fn collect(
    root: &Key,
    hive: &'static str,
    base: &str,
    depth_left: u32,
    out: &mut Vec<RegistrySnapshotEntry>,
) {
    let Ok(key) = root.open(base) else {
        return;
    };
    let Ok(subkeys) = key.keys() else {
        return;
    };
    for name in subkeys {
        let path = format!("{base}\\{name}");
        out.push(RegistrySnapshotEntry {
            hive,
            path: path.clone(),
        });
        if depth_left > 0 {
            collect(root, hive, &path, depth_left - 1, out);
        }
    }
}

/// Возвращает ключи из снимка, которые исчезли (были удалены).
pub fn diff_removed(before: &[RegistrySnapshotEntry]) -> Vec<RegistrySnapshotEntry> {
    before.iter().filter(|entry| !entry_exists(entry)).cloned().collect()
}

fn entry_exists(entry: &RegistrySnapshotEntry) -> bool {
    match entry.hive {
        "HKCU" => CURRENT_USER.open(&entry.path).is_ok(),
        _ => LOCAL_MACHINE.open(&entry.path).is_ok(),
    }
}

/// Удаляет ключ (вместе с поддеревом).
pub fn remove_entry(entry: &RegistrySnapshotEntry) -> Result<()> {
    match entry.hive {
        "HKCU" => CURRENT_USER.remove_tree(&entry.path),
        _ => LOCAL_MACHINE.remove_tree(&entry.path),
    }
    .with_context(|| format!("не удалось удалить ключ {}", entry.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_finds_software_roots() {
        let snapshot = capture_registry_snapshot();
        assert!(!snapshot.is_empty());
        assert!(snapshot.iter().any(|entry| entry.hive == "HKCU"));
        assert!(snapshot.iter().any(|entry| entry.hive == "HKLM"));
    }

    #[test]
    fn diff_reports_removed_entry() {
        let mut entry = RegistrySnapshotEntry {
            hive: "HKCU",
            path: r"Software\ChipClean\DoesNotExist".to_owned(),
        };
        // Создаём реальный тестовый ключ, затем удаляем, чтобы diff дал 1.
        let path = r"Software\ChipClean\TrackedTest";
        let _ = CURRENT_USER.remove_tree(path);
        CURRENT_USER.create(path).unwrap();
        entry.path = path.to_owned();

        let before = vec![entry.clone()];
        CURRENT_USER.remove_tree(path).unwrap();
        let removed = diff_removed(&before);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].path, path);

        let _ = CURRENT_USER.remove_tree(path);
    }
}
