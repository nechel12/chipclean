use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde_json::json;
use walkdir::WalkDir;

use crate::{
    config,
    core::{
        journal::Journal,
        registry_editor::{self, EditorHive, join_path},
    },
};

#[derive(Debug, Clone)]
pub struct FileSpec {
    pub pattern: String,
    pub recurse: bool,
}

#[derive(Debug, Clone)]
pub struct RegSpec {
    pub hive: EditorHive,
    pub path: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DataTarget {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub group: String,
    pub warning: Option<String>,
    pub default_enabled: bool,
    pub files: Vec<FileSpec>,
    pub registry: Vec<RegSpec>,
    pub exclude_files: Vec<String>,
    pub exclude_registry: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TargetScan {
    pub files: u64,
    pub bytes: u64,
    pub registry: u64,
    pub errors: u64,
}

#[derive(Debug, Default)]
pub struct CleanReport {
    pub files: u64,
    pub bytes: u64,
    pub registry: u64,
    pub errors: Vec<String>,
}

pub fn personal_targets() -> Vec<DataTarget> {
    vec![
        target(
            "recent",
            "Недавние документы",
            "Ярлыки и история недавних файлов",
            "Система и Проводник",
            true,
            None,
            vec![file(r"%APPDATA%\Microsoft\Windows\Recent", false)],
            vec![],
        ),
        target(
            "jumplists",
            "Списки переходов",
            "Jump Lists приложений и Explorer",
            "Система и Проводник",
            true,
            None,
            vec![
                file(
                    r"%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations",
                    true,
                ),
                file(
                    r"%APPDATA%\Microsoft\Windows\Recent\CustomDestinations",
                    true,
                ),
            ],
            vec![],
        ),
        target(
            "runmru",
            "История «Выполнить»",
            "Команды, введённые в окне Win+R",
            "Система и Проводник",
            true,
            None,
            vec![],
            vec![reg_key(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
            )],
        ),
        target(
            "typedpaths",
            "Введённые пути",
            "Адреса, введённые в проводнике",
            "Система и Проводник",
            true,
            None,
            vec![],
            vec![reg_key(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
            )],
        ),
        target(
            "recentdocs",
            "Список недавних документов",
            "RecentDocs в реестре пользователя",
            "Система и Проводник",
            true,
            None,
            vec![],
            vec![reg_key(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\RecentDocs",
            )],
        ),
        target(
            "search_history",
            "История поиска",
            "Поисковые запросы проводника",
            "Система и Проводник",
            true,
            None,
            vec![],
            vec![reg_key(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery",
            )],
        ),
        target(
            "notifications",
            "Кэш уведомлений",
            "База уведомлений Центра уведомлений",
            "Система и Проводник",
            false,
            Some("Уведомления могут пропасть до следующего входа в систему."),
            vec![file(
                r"%LOCALAPPDATA%\Microsoft\Windows\Notifications\wpndatabase.db",
                false,
            )],
            vec![],
        ),
        target(
            "edge_history",
            "Edge: история",
            "История посещений Microsoft Edge",
            "Браузеры",
            false,
            Some("Закройте браузер перед очисткой; активный профиль может воссоздать файл."),
            vec![
                file(r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\History", false),
                file(
                    r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\History-journal",
                    false,
                ),
            ],
            vec![],
        ),
        target(
            "edge_cookies",
            "Edge: куки",
            "Сессии сайтов Microsoft Edge",
            "Браузеры",
            false,
            Some("Все сайты потребуют повторного входа."),
            vec![file(
                r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\Network\Cookies",
                false,
            )],
            vec![],
        ),
        target(
            "chrome_history",
            "Chrome: история",
            "История посещений Google Chrome",
            "Браузеры",
            false,
            Some("Закройте браузер перед очисткой; активный профиль может воссоздать файл."),
            vec![
                file(r"%LOCALAPPDATA%\Google\Chrome\User Data\*\History", false),
                file(
                    r"%LOCALAPPDATA%\Google\Chrome\User Data\*\History-journal",
                    false,
                ),
            ],
            vec![],
        ),
        target(
            "chrome_cookies",
            "Chrome: куки",
            "Сессии сайтов Google Chrome",
            "Браузеры",
            false,
            Some("Все сайты потребуют повторного входа."),
            vec![file(
                r"%LOCALAPPDATA%\Google\Chrome\User Data\*\Network\Cookies",
                false,
            )],
            vec![],
        ),
        target(
            "firefox_history",
            "Firefox: история",
            "История посещений Mozilla Firefox",
            "Браузеры",
            false,
            Some("Закройте Firefox перед очисткой; активный профиль может воссоздать файл."),
            vec![
                file(r"%APPDATA%\Mozilla\Firefox\Profiles\*\places.sqlite", false),
                file(
                    r"%APPDATA%\Mozilla\Firefox\Profiles\*\places.sqlite-wal",
                    false,
                ),
            ],
            vec![],
        ),
        target(
            "firefox_cookies",
            "Firefox: куки",
            "Сессии сайтов Mozilla Firefox",
            "Браузеры",
            false,
            Some("Все сайты потребуют повторного входа."),
            vec![file(
                r"%APPDATA%\Mozilla\Firefox\Profiles\*\cookies.sqlite",
                false,
            )],
            vec![],
        ),
    ]
}

pub fn scan_target(target: &DataTarget) -> TargetScan {
    let mut scan = TargetScan::default();
    let mut seen = HashSet::new();
    for spec in &target.files {
        match collect_files(target, spec) {
            Ok(files) => {
                for (path, size) in files {
                    if seen.insert(path) {
                        scan.files += 1;
                        scan.bytes += size;
                    }
                }
            }
            Err(_) => scan.errors += 1,
        }
    }
    for spec in &target.registry {
        match scan_registry(spec, &target.exclude_registry) {
            Ok(count) => scan.registry += count,
            Err(_) => scan.errors += 1,
        }
    }
    scan
}

pub fn clean_target(target: &DataTarget) -> CleanReport {
    let mut report = CleanReport::default();
    let mut seen = HashSet::new();
    for spec in &target.files {
        match collect_files(target, spec) {
            Ok(files) => {
                for (path, size) in files {
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    let Ok(meta) = fs::symlink_metadata(&path) else {
                        report
                            .errors
                            .push(format!("{}: нет доступа", path.display()));
                        continue;
                    };
                    if !meta.file_type().is_file() {
                        continue;
                    }
                    match fs::remove_file(&path) {
                        Ok(()) => {
                            report.files += 1;
                            report.bytes += size;
                        }
                        Err(err) => report.errors.push(format!("{}: {err}", path.display())),
                    }
                }
            }
            Err(err) => report.errors.push(err.to_string()),
        }
    }

    for spec in &target.registry {
        if is_excluded_registry(spec, &target.exclude_registry) {
            continue;
        }
        match clean_registry(spec) {
            Ok(()) => report.registry += 1,
            Err(err) => report.errors.push(format!("{}: {err}", spec.path)),
        }
    }

    journal(json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "event": "privacy_clean",
        "target": target.id,
        "files": report.files,
        "bytes": report.bytes,
        "registry": report.registry,
        "errors": report.errors.len(),
    }));
    report
}

pub fn expand_pattern(pattern: &str) -> Vec<PathBuf> {
    let expanded = expand_env(pattern);
    if expanded.contains('*') || expanded.contains('?') {
        let pattern = expanded.replace('\\', "/");
        return glob::glob(&pattern)
            .map(|entries| entries.filter_map(|entry| entry.ok()).collect())
            .unwrap_or_default();
    }
    let path = PathBuf::from(expanded);
    path.exists().then_some(path).into_iter().collect()
}

fn collect_files(target: &DataTarget, spec: &FileSpec) -> Result<Vec<(PathBuf, u64)>> {
    let mut files = Vec::new();
    for root in expand_pattern(&spec.pattern) {
        if root.is_file() {
            if !is_excluded(&root, &target.exclude_files) {
                let size = fs::metadata(&root).map(|meta| meta.len()).unwrap_or(0);
                files.push((root, size));
            }
            continue;
        }
        if !root.is_dir() {
            continue;
        }
        let walker = if spec.recurse {
            WalkDir::new(&root)
        } else {
            WalkDir::new(&root).max_depth(1)
        };
        for entry in walker.into_iter().filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if !entry.file_type().is_file() || is_excluded(path, &target.exclude_files) {
                continue;
            }
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            files.push((path.to_path_buf(), size));
        }
    }
    Ok(files)
}

fn scan_registry(spec: &RegSpec, excludes: &[String]) -> Result<u64> {
    if is_excluded_registry(spec, excludes) {
        return Ok(0);
    }
    if let Some(value) = &spec.value {
        let Ok(key) = registry_editor::open_key(spec.hive, &spec.path) else {
            return Ok(0);
        };
        return Ok(u64::from(key.get_value(value).is_ok()));
    }

    let mut queue = vec![spec.path.clone()];
    let mut count = 0_u64;
    while let Some(path) = queue.pop() {
        if count > 10_000 {
            break;
        }
        let Ok(key) = registry_editor::open_key(spec.hive, &path) else {
            continue;
        };
        count += key.values().map(|values| values.count()).unwrap_or(0) as u64;
        if let Ok(keys) = key.keys() {
            for child in keys {
                queue.push(join_path(&path, &child));
            }
        }
    }
    Ok(count)
}

fn clean_registry(spec: &RegSpec) -> Result<()> {
    if !matches!(spec.hive, EditorHive::Hkcu | EditorHive::Hklm) {
        anyhow::bail!("очистка поддерживается только для HKCU и HKLM");
    }
    if let Some(value) = &spec.value {
        let Ok(key) = registry_editor::open_key(spec.hive, &spec.path) else {
            return Ok(());
        };
        if key.get_value(value).is_err() {
            return Ok(());
        }
        registry_editor::delete_value(spec.hive, &spec.path, value, &config::backups_dir())?;
    } else {
        if registry_editor::open_key(spec.hive, &spec.path).is_err() {
            return Ok(());
        }
        registry_editor::delete_key(spec.hive, &spec.path, &config::backups_dir())?;
    }
    Ok(())
}

fn is_excluded(path: &Path, excludes: &[String]) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/").to_lowercase();
    excludes.iter().any(|pattern| {
        let pattern = expand_env(pattern).replace('\\', "/").to_lowercase();
        glob::Pattern::new(&pattern)
            .map(|pattern| pattern.matches(&normalized))
            .unwrap_or(false)
    })
}

fn is_excluded_registry(spec: &RegSpec, excludes: &[String]) -> bool {
    let full = format!("{}\\{}", spec.hive.short(), spec.path).to_lowercase();
    excludes.iter().any(|exclude| {
        let exclude = exclude.to_lowercase();
        full.starts_with(&exclude) || full == exclude
    })
}

fn expand_env(input: &str) -> String {
    let mut result = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        let (before, after) = rest.split_at(start);
        result.push_str(before);
        let after = &after[1..];
        if let Some(end) = after.find('%') {
            let name = &after[..end];
            if let Some(value) = std::env::var_os(name) {
                result.push_str(&value.to_string_lossy());
            } else {
                result.push('%');
                result.push_str(name);
                result.push('%');
            }
            rest = &after[end + 1..];
        } else {
            result.push('%');
            result.push_str(after);
            rest = "";
        }
    }
    result.push_str(rest);
    result
}

fn file(pattern: &str, recurse: bool) -> FileSpec {
    FileSpec {
        pattern: pattern.to_owned(),
        recurse,
    }
}

fn reg_key(path: &str) -> RegSpec {
    RegSpec {
        hive: EditorHive::Hkcu,
        path: path.to_owned(),
        value: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn target(
    id: &str,
    title: &str,
    summary: &str,
    group: &str,
    default_enabled: bool,
    warning: Option<&str>,
    files: Vec<FileSpec>,
    registry: Vec<RegSpec>,
) -> DataTarget {
    DataTarget {
        id: id.to_owned(),
        title: title.to_owned(),
        summary: summary.to_owned(),
        group: group.to_owned(),
        warning: warning.map(str::to_owned),
        default_enabled,
        files,
        registry,
        exclude_files: Vec::new(),
        exclude_registry: Vec::new(),
    }
}

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write privacy journal event");
            } else {
                let _ = journal.flush();
            }
        }
        Err(err) => tracing::warn!(%err, "failed to open operations journal"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_environment() {
        let expanded = expand_env(r"%TEMP%\ChipCleanTest");
        assert!(expanded.to_lowercase().contains("temp"));
        assert!(!expanded.contains('%'));
    }

    #[test]
    fn target_scan_finds_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        fs::write(&path, b"data").unwrap();
        let target = target(
            "test",
            "Test",
            "Test",
            "Test",
            true,
            None,
            vec![file(&path.display().to_string(), false)],
            vec![],
        );
        let scan = scan_target(&target);
        assert_eq!(scan.files, 1);
        assert_eq!(scan.bytes, 4);
    }

    #[test]
    fn cleans_a_file_and_registry_value() {
        use windows_registry::CURRENT_USER;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("personal.dat");
        fs::write(&path, b"personal").unwrap();
        let unique = format!(
            "ChipCleanPrivacy-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros()
        );
        let key_path = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&key_path).unwrap();
        key.set_string("Value", "test").unwrap();
        let target = target(
            "test",
            "Test",
            "Test",
            "Test",
            true,
            None,
            vec![file(&path.display().to_string(), false)],
            vec![RegSpec {
                hive: EditorHive::Hkcu,
                path: key_path.clone(),
                value: Some("Value".to_owned()),
            }],
        );

        let report = clean_target(&target);
        assert_eq!(report.files, 1);
        assert_eq!(report.registry, 1);
        assert!(!path.exists());
        assert!(key.get_value("Value").is_err());

        drop(key);
        let _ = CURRENT_USER.remove_tree(&key_path);
    }
}
