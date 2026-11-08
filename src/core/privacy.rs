use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use walkdir::WalkDir;
use windows_registry::Key;

use crate::{
    config,
    core::{
        journal::Journal,
        registry_editor::{self, EditorHive, full_path, join_path},
        safety::{self, Risk, expand_env},
    },
};

#[derive(Debug, Clone)]
pub struct FileSpec {
    pub pattern: String,
    pub recurse: bool,
}

#[derive(Debug, Clone)]
pub struct HistorySpec {
    pub pattern: String,
}

#[derive(Debug, Clone)]
pub struct RegSpec {
    pub hive: EditorHive,
    pub path: String,
    pub value: Option<String>,
    pub recurse: bool,
    pub remove_self: bool,
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
    pub history: Vec<HistorySpec>,
    pub registry: Vec<RegSpec>,
    pub exclude_files: Vec<String>,
    pub exclude_registry: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TargetScan {
    pub files: u64,
    pub bytes: u64,
    pub history: u64,
    pub registry: u64,
    pub registry_items: Vec<String>,
    pub risky_count: u64,
    pub risky_items: Vec<String>,
    pub blocked_count: u64,
    pub blocked_items: Vec<String>,
    pub errors: u64,
}

#[derive(Debug, Default)]
pub struct CleanReport {
    pub files: u64,
    pub bytes: u64,
    pub history: u64,
    pub registry: u64,
    pub errors: Vec<String>,
}

impl CleanReport {
    pub fn merge(&mut self, other: CleanReport) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.history += other.history;
        self.registry += other.registry;
        self.errors.extend(other.errors);
    }
}

fn record_blocked(scan: &mut TargetScan, item: String) {
    scan.blocked_count += 1;
    push_preview(&mut scan.blocked_items, item);
}

fn record_risky(scan: &mut TargetScan, item: String) {
    scan.risky_count += 1;
    push_preview(&mut scan.risky_items, item);
}

fn push_preview(list: &mut Vec<String>, item: String) {
    if list.len() < safety::PREVIEW_LIMIT {
        list.push(item);
    }
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
        history_target(
            "firefox_history",
            "Firefox: история",
            "История посещений; закладки сохраняются",
            "Браузеры",
            false,
            Some(
                "Закройте Firefox перед очисткой; история посещений будет удалена, закладки сохранятся.",
            ),
            vec![r"%APPDATA%\Mozilla\Firefox\Profiles\*\places.sqlite"],
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
        let pattern_risk = safety::classify_file_pattern(&spec.pattern);
        if pattern_risk == Risk::Blocked {
            record_blocked(&mut scan, format!("{} (путь защищён)", spec.pattern));
            continue;
        }
        match collect_files(target, spec) {
            Ok(files) => {
                for (path, size) in files {
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    let file_risk = safety::classify_file(&path);
                    if file_risk == Risk::Blocked {
                        record_blocked(&mut scan, format!("{} (путь защищён)", path.display()));
                        continue;
                    }
                    if file_risk == Risk::Outside {
                        record_risky(&mut scan, path.display().to_string());
                    }
                    scan.files += 1;
                    scan.bytes += size;
                }
            }
            Err(_) => scan.errors += 1,
        }
    }
    for spec in &target.history {
        let (visits, errors) = scan_history(spec);
        scan.history += visits;
        scan.errors += errors;
    }
    for spec in &target.registry {
        let risk = safety::classify_registry(spec.hive, &spec.path, spec.remove_self);
        if risk == Risk::Blocked {
            record_blocked(
                &mut scan,
                format!("{} (раздел защищён)", full_path(spec.hive, &spec.path)),
            );
            continue;
        }
        match scan_registry(spec, &target.exclude_registry) {
            Ok((count, items)) => {
                scan.registry += count;
                if risk == Risk::Outside {
                    scan.risky_count += count;
                    for item in &items {
                        push_preview(&mut scan.risky_items, item.clone());
                    }
                }
                let remaining = REGISTRY_PREVIEW_LIMIT.saturating_sub(scan.registry_items.len());
                scan.registry_items
                    .extend(items.into_iter().take(remaining));
            }
            Err(_) => scan.errors += 1,
        }
    }
    scan
}

pub fn clean_target(target: &DataTarget) -> CleanReport {
    let mut report = CleanReport::default();
    let mut seen = HashSet::new();
    for spec in &target.files {
        if safety::classify_file_pattern(&spec.pattern) == Risk::Blocked {
            report
                .errors
                .push(format!("{}: защищённый путь пропущен", spec.pattern));
            continue;
        }
        match collect_files(target, spec) {
            Ok(files) => {
                for (path, size) in files {
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    if safety::classify_file(&path) == Risk::Blocked {
                        report
                            .errors
                            .push(format!("{}: защищённый путь пропущен", path.display()));
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

    for spec in &target.history {
        let (visits, errors) = clean_history(spec);
        report.history += visits;
        report.errors.extend(errors);
    }

    for spec in &target.registry {
        if safety::classify_registry(spec.hive, &spec.path, spec.remove_self) == Risk::Blocked {
            report.errors.push(format!(
                "{}: защищённый раздел пропущен",
                full_path(spec.hive, &spec.path)
            ));
            continue;
        }
        let (deleted, errors) = clean_registry(spec, &target.exclude_registry);
        report.registry += deleted;
        report.errors.extend(errors);
    }

    if let Some(err) = journal(json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "event": "privacy_clean",
        "target": target.id,
        "files": report.files,
        "bytes": report.bytes,
        "history": report.history,
        "registry": report.registry,
        "errors": report.errors.len(),
    })) {
        report
            .errors
            .push(format!("не удалось записать журнал: {err}"));
    }
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
            WalkDir::new(&root).follow_root_links(false)
        } else {
            WalkDir::new(&root).max_depth(1).follow_root_links(false)
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

fn scan_history(spec: &HistorySpec) -> (u64, u64) {
    let mut visits = 0;
    let mut errors = 0;
    let mut seen = HashSet::new();
    for path in expand_pattern(&spec.pattern) {
        if !path.is_file() || !seen.insert(path.clone()) {
            continue;
        }
        match count_visits(&path) {
            Ok(count) => visits += count,
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "failed to scan Firefox history");
                errors += 1;
            }
        }
    }
    (visits, errors)
}

fn clean_history(spec: &HistorySpec) -> (u64, Vec<String>) {
    let mut visits = 0;
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    for path in expand_pattern(&spec.pattern) {
        if !path.is_file() || !seen.insert(path.clone()) {
            continue;
        }
        match clean_history_file(&path) {
            Ok(count) => visits += count,
            Err(err) => errors.push(format!("{}: {err:#}", path.display())),
        }
    }
    (visits, errors)
}

fn count_visits(path: &Path) -> Result<u64> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("не удалось открыть {}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(2))?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM moz_historyvisits", [], |row| {
        row.get(0)
    })?;
    Ok(count.max(0) as u64)
}

fn clean_history_file(path: &Path) -> Result<u64> {
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("не удалось открыть {}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))?;
    if !table_exists(&conn, "moz_bookmarks")? {
        bail!("файл не похож на базу Firefox");
    }
    if !table_exists(&conn, "moz_historyvisits")? {
        return Ok(0);
    }

    let tx = conn.transaction()?;
    let deleted = tx.execute("DELETE FROM moz_historyvisits", [])?;
    tx.commit()?;
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    Ok(deleted as u64)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

const MAX_REGISTRY_KEYS: usize = 20_000;
const MAX_REGISTRY_OBJECTS: usize = 50_000;
const REGISTRY_PREVIEW_LIMIT: usize = 200;

#[derive(Default)]
struct RegistryPreview {
    count: u64,
    items: Vec<String>,
}

impl RegistryPreview {
    fn add(&mut self, item: String) {
        if self.is_full() {
            return;
        }
        self.count += 1;
        if self.items.len() < REGISTRY_PREVIEW_LIMIT {
            self.items.push(item);
        }
    }

    fn is_full(&self) -> bool {
        self.count >= MAX_REGISTRY_OBJECTS as u64
    }
}

fn scan_registry(spec: &RegSpec, excludes: &[String]) -> Result<(u64, Vec<String>)> {
    if is_excluded_registry(spec, excludes) {
        return Ok((0, Vec::new()));
    }
    let mut preview = RegistryPreview::default();
    if spec.remove_self {
        if has_excluded_descendant(spec.hive, &spec.path, excludes) {
            return Ok((0, Vec::new()));
        }
        for (path, key) in collect_registry_keys(spec.hive, &spec.path, true, excludes) {
            if preview.is_full() {
                break;
            }
            preview.add(format!("{} (раздел)", full_path(spec.hive, &path)));
            add_key_values(&mut preview, spec.hive, &path, &key);
        }
    } else {
        for (path, key) in collect_registry_keys(spec.hive, &spec.path, spec.recurse, excludes) {
            if preview.is_full() {
                break;
            }
            if let Some(value) = &spec.value {
                if key.get_value(value).is_ok() {
                    preview.add(registry_value_path(spec.hive, &path, value));
                }
            } else {
                add_key_values(&mut preview, spec.hive, &path, &key);
            }
        }
    }
    Ok((preview.count, preview.items))
}

fn clean_registry(spec: &RegSpec, excludes: &[String]) -> (u64, Vec<String>) {
    if !matches!(spec.hive, EditorHive::Hkcu | EditorHive::Hklm) {
        return (
            0,
            vec!["очистка поддерживается только для HKCU и HKLM".to_owned()],
        );
    }
    if is_excluded_registry(spec, excludes) {
        return (0, Vec::new());
    }
    if spec.remove_self {
        if has_excluded_descendant(spec.hive, &spec.path, excludes) {
            return (
                0,
                vec![format!(
                    "{}: раздел не удалён, потому что содержит исключённые подключи",
                    full_path(spec.hive, &spec.path)
                )],
            );
        }
        let count = scan_registry(spec, excludes)
            .map(|(count, _)| count)
            .unwrap_or(0);
        if registry_editor::open_key(spec.hive, &spec.path).is_err() {
            return (0, Vec::new());
        }
        return match registry_editor::delete_key(spec.hive, &spec.path, &config::backups_dir()) {
            Ok(_) => (count, Vec::new()),
            Err(err) => (0, vec![format!("{}: {err}", spec.path)]),
        };
    }

    let items = collect_value_deletions(spec, excludes);
    if items.is_empty() {
        return (0, Vec::new());
    }
    if let Err(err) = registry_editor::backup_key_to(spec.hive, &spec.path, &config::backups_dir())
    {
        return (0, vec![format!("{}: {err}", spec.path)]);
    }

    let mut deleted = 0;
    let mut errors = Vec::new();
    for (path, name) in items {
        match registry_editor::remove_value_raw(spec.hive, &path, &name) {
            Ok(()) => deleted += 1,
            Err(err) => errors.push(format!(
                "{}: {err}",
                registry_value_path(spec.hive, &path, &name)
            )),
        }
    }
    (deleted, errors)
}

fn collect_value_deletions(spec: &RegSpec, excludes: &[String]) -> Vec<(String, String)> {
    let mut items = Vec::new();
    for (path, key) in collect_registry_keys(spec.hive, &spec.path, spec.recurse, excludes) {
        if items.len() >= MAX_REGISTRY_OBJECTS {
            break;
        }
        if let Some(value) = &spec.value {
            if key.get_value(value).is_ok() {
                items.push((path, value.clone()));
            }
        } else if let Ok(values) = key.values() {
            for (name, _) in values {
                if items.len() >= MAX_REGISTRY_OBJECTS {
                    break;
                }
                items.push((path.clone(), name));
            }
        }
    }
    items
}

fn collect_registry_keys(
    hive: EditorHive,
    root: &str,
    recurse: bool,
    excludes: &[String],
) -> Vec<(String, Key)> {
    let mut keys = Vec::new();
    let mut queue = VecDeque::from([root.to_owned()]);
    let mut visited = HashSet::new();
    while let Some(path) = queue.pop_front() {
        if keys.len() >= MAX_REGISTRY_KEYS || !visited.insert(path.clone()) {
            break;
        }
        if is_excluded_registry_path(hive, &path, excludes) {
            continue;
        }
        let Ok(key) = registry_editor::open_key(hive, &path) else {
            continue;
        };
        if recurse && let Ok(children) = key.keys() {
            for child in children {
                queue.push_back(join_path(&path, &child));
            }
        }
        keys.push((path, key));
    }
    keys
}

fn add_key_values(preview: &mut RegistryPreview, hive: EditorHive, path: &str, key: &Key) {
    let Ok(values) = key.values() else {
        return;
    };
    for (name, _) in values {
        if preview.is_full() {
            break;
        }
        preview.add(registry_value_path(hive, path, &name));
    }
}

fn registry_value_path(hive: EditorHive, path: &str, name: &str) -> String {
    let label = if name.is_empty() {
        "(По умолчанию)"
    } else {
        name
    };
    format!("{}\\{label}", full_path(hive, path))
}

fn has_excluded_descendant(hive: EditorHive, root: &str, excludes: &[String]) -> bool {
    let root = full_path(hive, root).trim_end_matches('\\').to_lowercase();
    let prefix = format!("{root}\\");
    excludes.iter().any(|exclude| {
        let exclude = exclude.trim().trim_end_matches('\\').to_lowercase();
        exclude.starts_with(&prefix)
    })
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
    is_excluded_registry_path(spec.hive, &spec.path, excludes)
}

fn is_excluded_registry_path(hive: EditorHive, path: &str, excludes: &[String]) -> bool {
    let full = full_path(hive, path).trim_end_matches('\\').to_lowercase();
    excludes.iter().any(|exclude| {
        let exclude = exclude.trim().trim_end_matches('\\').to_lowercase();
        if exclude.is_empty() {
            return false;
        }
        full == exclude || full.starts_with(&format!("{exclude}\\"))
    })
}

fn file(pattern: &str, recurse: bool) -> FileSpec {
    FileSpec {
        pattern: pattern.to_owned(),
        recurse,
    }
}

fn history_spec(pattern: &str) -> HistorySpec {
    HistorySpec {
        pattern: pattern.to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn history_target(
    id: &str,
    title: &str,
    summary: &str,
    group: &str,
    default_enabled: bool,
    warning: Option<&str>,
    patterns: Vec<&str>,
) -> DataTarget {
    let mut target = target(
        id,
        title,
        summary,
        group,
        default_enabled,
        warning,
        Vec::new(),
        Vec::new(),
    );
    target.history = patterns.into_iter().map(history_spec).collect();
    target
}

fn reg_key(path: &str) -> RegSpec {
    RegSpec {
        hive: EditorHive::Hkcu,
        path: path.to_owned(),
        value: None,
        recurse: true,
        remove_self: true,
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
        history: Vec::new(),
        registry,
        exclude_files: Vec::new(),
        exclude_registry: Vec::new(),
    }
}

fn journal(event: serde_json::Value) -> Option<String> {
    match Journal::open() {
        Ok(mut journal) => journal
            .record(event)
            .and_then(|()| journal.flush())
            .err()
            .map(|err| {
                tracing::warn!(%err, "failed to write privacy journal event");
                err.to_string()
            }),
        Err(err) => {
            tracing::warn!(%err, "failed to open operations journal");
            Some(err.to_string())
        }
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
    fn cleans_firefox_visits_and_keeps_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("places.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT);
             CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, fk INTEGER, title TEXT);
             CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, place_id INTEGER, visit_date INTEGER);
             INSERT INTO moz_places (id, url) VALUES (1, 'https://example.com');
             INSERT INTO moz_bookmarks (id, fk, title) VALUES (1, 1, 'Example');
             INSERT INTO moz_historyvisits (place_id, visit_date) VALUES (1, 123);",
        )
        .unwrap();
        drop(conn);

        let pattern = path.display().to_string();
        let target = history_target(
            "test",
            "Test",
            "Test",
            "Test",
            true,
            None,
            vec![pattern.as_str()],
        );
        let scan = scan_target(&target);
        assert_eq!(scan.history, 1);
        assert_eq!(scan.errors, 0);

        let report = clean_target(&target);
        assert_eq!(report.history, 1);
        assert!(report.errors.is_empty());
        assert!(path.exists());

        let conn = Connection::open(&path).unwrap();
        let visits: i64 = conn
            .query_row("SELECT COUNT(*) FROM moz_historyvisits", [], |row| {
                row.get(0)
            })
            .unwrap();
        let bookmarks: i64 = conn
            .query_row("SELECT COUNT(*) FROM moz_bookmarks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(visits, 0);
        assert_eq!(bookmarks, 1);
    }

    #[test]
    fn refuses_to_clean_a_foreign_sqlite_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foreign.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);")
            .unwrap();
        drop(conn);

        let pattern = path.display().to_string();
        let target = history_target(
            "test",
            "Test",
            "Test",
            "Test",
            true,
            None,
            vec![pattern.as_str()],
        );
        let report = clean_target(&target);
        assert_eq!(report.history, 0);
        assert_eq!(report.errors.len(), 1);
        assert!(path.exists());
    }

    #[test]
    fn cleans_a_file_and_registry_value() {
        use windows_registry::CURRENT_USER;

        let _guard = crate::core::registry_test_guard();
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
                recurse: false,
                remove_self: false,
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

    #[test]
    fn deletes_all_values_without_removing_the_key() {
        use windows_registry::CURRENT_USER;

        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanPrivacyAll-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros()
        );
        let key_path = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&key_path).unwrap();
        key.set_string("One", "1").unwrap();
        key.set_string("Two", "2").unwrap();
        let target = target(
            "test",
            "Test",
            "Test",
            "Test",
            true,
            None,
            vec![],
            vec![RegSpec {
                hive: EditorHive::Hkcu,
                path: key_path.clone(),
                value: None,
                recurse: false,
                remove_self: false,
            }],
        );

        let scan = scan_target(&target);
        assert_eq!(scan.registry, 2);
        assert_eq!(scan.registry_items.len(), 2);

        let report = clean_target(&target);
        assert_eq!(report.registry, 2);
        assert!(report.errors.is_empty());
        assert!(key.get_value("One").is_err());
        assert!(key.get_value("Two").is_err());
        assert!(CURRENT_USER.open(&key_path).is_ok());

        drop(key);
        let _ = CURRENT_USER.remove_tree(&key_path);
    }

    #[test]
    fn recurses_into_subkeys_for_a_specific_value() {
        use windows_registry::CURRENT_USER;

        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanPrivacyRecurse-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros()
        );
        let key_path = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&key_path).unwrap();
        key.set_string("Target", "root").unwrap();
        let child = key.create("Child").unwrap();
        child.set_string("Target", "child").unwrap();
        child.set_string("Keep", "1").unwrap();
        let target = target(
            "test",
            "Test",
            "Test",
            "Test",
            true,
            None,
            vec![],
            vec![RegSpec {
                hive: EditorHive::Hkcu,
                path: key_path.clone(),
                value: Some("Target".to_owned()),
                recurse: true,
                remove_self: false,
            }],
        );

        let scan = scan_target(&target);
        assert_eq!(scan.registry, 2);

        let report = clean_target(&target);
        assert_eq!(report.registry, 2);
        assert!(key.get_value("Target").is_err());
        assert!(child.get_value("Target").is_err());
        assert!(child.get_value("Keep").is_ok());

        drop(child);
        drop(key);
        let _ = CURRENT_USER.remove_tree(&key_path);
    }

    #[test]
    fn remove_self_deletes_the_key_tree() {
        use windows_registry::CURRENT_USER;

        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanPrivacyTree-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros()
        );
        let key_path = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&key_path).unwrap();
        key.set_string("One", "1").unwrap();
        let child = key.create("Child").unwrap();
        child.set_string("Keep", "1").unwrap();
        drop(child);
        drop(key);

        let target = target(
            "test",
            "Test",
            "Test",
            "Test",
            true,
            None,
            vec![],
            vec![RegSpec {
                hive: EditorHive::Hkcu,
                path: key_path.clone(),
                value: None,
                recurse: true,
                remove_self: true,
            }],
        );

        let scan = scan_target(&target);
        assert_eq!(scan.registry, 4);

        let report = clean_target(&target);
        assert_eq!(report.registry, 4);
        assert!(report.errors.is_empty());
        assert!(CURRENT_USER.open(&key_path).is_err());
    }
}
