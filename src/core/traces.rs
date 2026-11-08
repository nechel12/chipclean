use std::{
    collections::BTreeSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use crossbeam_channel::Sender;
use egui_phosphor::regular as ph;
use serde::{Deserialize, Serialize};
use serde_json::json;
use walkdir::WalkDir;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{Interface, PCWSTR};

use crate::{
    config,
    core::{
        fs_atomic, fs_move,
        journal::Journal,
        model::Risk,
        path_guard,
        uninstall::{self, ProgramCommand, ProgramEntry},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TraceCategoryId {
    BrokenShortcuts,
    OrphanedFolders,
    DeepLeftovers,
}

impl TraceCategoryId {
    pub fn key(self) -> &'static str {
        match self {
            Self::BrokenShortcuts => "broken_shortcuts",
            Self::OrphanedFolders => "orphaned_folders",
            Self::DeepLeftovers => "deep_leftovers",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TraceCategory {
    pub id: TraceCategoryId,
    pub title: &'static str,
    pub summary: &'static str,
    pub icon: &'static str,
    pub risk: Risk,
    pub default_enabled: bool,
    pub experimental: bool,
}

pub fn catalog() -> Vec<TraceCategory> {
    vec![
        TraceCategory {
            id: TraceCategoryId::BrokenShortcuts,
            title: "Битые ярлыки",
            summary: "Ссылки на отсутствующие файлы в меню «Пуск» и на рабочем столе",
            icon: ph::LINK_SIMPLE_BREAK,
            risk: Risk::Caution,
            default_enabled: true,
            experimental: false,
        },
        TraceCategory {
            id: TraceCategoryId::OrphanedFolders,
            title: "Осиротевшие папки программ",
            summary: "Деинсталлятор есть, записи Uninstall нет",
            icon: ph::FOLDER_SIMPLE,
            risk: Risk::Advanced,
            default_enabled: true,
            experimental: false,
        },
        TraceCategory {
            id: TraceCategoryId::DeepLeftovers,
            title: "Глубокий поиск (тест)",
            summary: "AppData и ProgramData · возможны ошибки",
            icon: ph::MAGNIFYING_GLASS_PLUS,
            risk: Risk::Advanced,
            default_enabled: false,
            experimental: true,
        },
    ]
}

#[derive(Debug, Clone)]
pub enum TraceKind {
    Shortcut { target: PathBuf },
    OrphanFolder { evidence: PathBuf },
    DataFolder { reason: String },
}

#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub category: TraceCategoryId,
    pub title: String,
    pub detail: String,
    pub path: PathBuf,
    pub kind: TraceKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuarantinedTrace {
    pub category: TraceCategoryId,
    pub title: String,
    pub detail: String,
    pub original_path: PathBuf,
    pub stored_path: PathBuf,
    pub quarantined_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TraceStore {
    pub entries: Vec<QuarantinedTrace>,
}

#[derive(Debug)]
pub enum TraceScanEvent {
    Started {
        total: usize,
    },
    CategoryStarted {
        id: TraceCategoryId,
    },
    CategoryFinished {
        entries: Vec<TraceEntry>,
        error: Option<String>,
    },
    Finished {
        total: usize,
        elapsed: Duration,
        cancelled: bool,
    },
}

pub fn spawn_scan(
    categories: Vec<TraceCategoryId>,
    cancel: Arc<AtomicBool>,
    tx: Sender<TraceScanEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("chipclean-traces-scan".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let total_categories = categories.len();
            let mut total_findings = 0_usize;

            let _ = tx.send(TraceScanEvent::Started {
                total: total_categories,
            });

            for id in categories {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }

                if tx.send(TraceScanEvent::CategoryStarted { id }).is_err() {
                    return;
                }

                let result = match id {
                    TraceCategoryId::BrokenShortcuts => scan_shortcuts(&cancel),
                    TraceCategoryId::OrphanedFolders => scan_orphaned_folders(&cancel),
                    TraceCategoryId::DeepLeftovers => scan_deep_leftovers(&cancel),
                };

                match result {
                    Ok(entries) => {
                        total_findings += entries.len();
                        if tx
                            .send(TraceScanEvent::CategoryFinished {
                                entries,
                                error: None,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(err) => {
                        if tx
                            .send(TraceScanEvent::CategoryFinished {
                                entries: Vec::new(),
                                error: Some(err.to_string()),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }

            let _ = tx.send(TraceScanEvent::Finished {
                total: total_findings,
                elapsed: started.elapsed(),
                cancelled: cancel.load(Ordering::Relaxed),
            });
        })
        .expect("не удалось создать поток поиска следов")
}

fn scan_shortcuts(cancel: &AtomicBool) -> Result<Vec<TraceEntry>> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.0 >= 0;
    if !initialized {
        bail!("не удалось инициализировать COM для чтения ярлыков");
    }

    let result = (|| -> Result<Vec<TraceEntry>> {
        let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
            .context("не удалось создать IShellLink")?;
        let persist: IPersistFile = link.cast().context("не удалось получить IPersistFile")?;
        let mut entries = Vec::new();

        for path in shortcut_files() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let Ok(Some(target)) = resolve_shortcut(&link, &persist, &path) else {
                continue;
            };
            if target.exists() || !root_exists(&target) || is_windows_apps(&target) {
                continue;
            }

            entries.push(TraceEntry {
                category: TraceCategoryId::BrokenShortcuts,
                title: path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string()),
                detail: format!("Цель не найдена: {}", target.display()),
                path,
                kind: TraceKind::Shortcut { target },
            });
        }

        Ok(entries)
    })();

    unsafe {
        CoUninitialize();
    }

    result
}

fn resolve_shortcut(
    link: &IShellLinkW,
    persist: &IPersistFile,
    path: &Path,
) -> Result<Option<PathBuf>> {
    let wide = crate::win::wide_os(path.as_os_str());
    unsafe { persist.Load(PCWSTR(wide.as_ptr()), STGM_READ) }
        .context("не удалось прочитать ярлык")?;

    let mut buffer = [0_u16; 32768];
    unsafe { link.GetPath(&mut buffer, std::ptr::null_mut(), 0) }
        .context("не удалось получить цель ярлыка")?;

    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    if length == 0 {
        return Ok(None);
    }

    let raw = String::from_utf16_lossy(&buffer[..length]);
    let expanded = expand_env(&raw);
    let target = PathBuf::from(expanded.trim_matches('"'));

    if !target.is_absolute() || target.to_string_lossy().starts_with(r"\\") {
        return Ok(None);
    }

    Ok(Some(target))
}

fn shortcut_files() -> Vec<PathBuf> {
    let mut files = BTreeSet::new();

    for root in start_menu_roots() {
        collect_lnk_recursive(&root, &mut files);
    }
    for root in desktop_roots() {
        collect_lnk_top_level(&root, &mut files);
    }

    files.into_iter().collect()
}

fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(programdata) = std::env::var_os("ProgramData") {
        roots.push(PathBuf::from(programdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }

    roots
}

fn desktop_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(userprofile) = std::env::var_os("USERPROFILE") {
        roots.push(PathBuf::from(userprofile).join("Desktop"));
    }
    if let Some(public) = std::env::var_os("PUBLIC") {
        roots.push(PathBuf::from(public).join("Desktop"));
    }

    roots
}

fn trace_original_roots(category: TraceCategoryId) -> Vec<PathBuf> {
    match category {
        TraceCategoryId::BrokenShortcuts => {
            let mut roots = start_menu_roots();
            roots.extend(desktop_roots());
            roots
        }
        TraceCategoryId::OrphanedFolders => program_files_roots(),
        TraceCategoryId::DeepLeftovers => appdata_roots(),
    }
}

fn collect_lnk_recursive(root: &Path, files: &mut BTreeSet<PathBuf>) {
    if !root.is_dir() {
        return;
    }

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_visit)
        .flatten()
    {
        if entry.file_type().is_file() && is_lnk(entry.path()) {
            files.insert(entry.path().to_path_buf());
        }
    }
}

fn collect_lnk_top_level(root: &Path, files: &mut BTreeSet<PathBuf>) {
    let Ok(read_dir) = fs::read_dir(root) else {
        return;
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_file() && is_lnk(&path) {
            files.insert(path);
        }
    }
}

fn is_lnk(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false)
}

fn scan_orphaned_folders(cancel: &AtomicBool) -> Result<Vec<TraceEntry>> {
    let programs =
        uninstall::scan_programs().context("не удалось сопоставить установленные программы")?;
    let references = installed_references(&programs);
    let mut entries = Vec::new();

    for root in program_files_roots() {
        let Ok(read_dir) = fs::read_dir(&root) else {
            continue;
        };

        for entry in read_dir.flatten() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_lowercase();
            if is_system_folder(&name) || is_referenced(&path, &references) {
                continue;
            }

            let Some(evidence) = find_uninstaller(&path) else {
                continue;
            };

            entries.push(TraceEntry {
                category: TraceCategoryId::OrphanedFolders,
                title: entry.file_name().to_string_lossy().to_string(),
                detail: format!(
                    "Найден деинсталлятор без записи Uninstall: {}",
                    evidence.display()
                ),
                path,
                kind: TraceKind::OrphanFolder { evidence },
            });
        }
    }

    Ok(entries)
}

fn scan_deep_leftovers(cancel: &AtomicBool) -> Result<Vec<TraceEntry>> {
    let programs =
        uninstall::scan_programs().context("не удалось сопоставить установленные программы")?;
    let references = installed_references(&programs);
    let installed_tokens = installed_name_tokens(&programs);
    let mut entries = Vec::new();

    for root in appdata_roots() {
        let Ok(read_dir) = fs::read_dir(&root) else {
            continue;
        };

        for entry in read_dir.flatten() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if is_system_folder(&lower)
                || is_referenced(&path, &references)
                || folder_matches_installed(&lower, &installed_tokens)
            {
                continue;
            }

            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            let Ok(age) = SystemTime::now().duration_since(modified) else {
                continue;
            };
            if age < Duration::from_secs(30 * 86_400) {
                continue;
            }

            entries.push(TraceEntry {
                category: TraceCategoryId::DeepLeftovers,
                title: name,
                detail: "Папка данных без установленной программы · тестовая эвристика".to_owned(),
                path,
                kind: TraceKind::DataFolder {
                    reason: "Нет установленной программы или ссылок на эту папку".to_owned(),
                },
            });
        }
    }

    Ok(entries)
}

fn appdata_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    for variable in ["APPDATA", "LOCALAPPDATA", "ProgramData"] {
        if let Some(path) = std::env::var_os(variable) {
            let path = PathBuf::from(path);
            if path.is_dir()
                && !roots
                    .iter()
                    .any(|root| normalize_path(root) == normalize_path(&path))
            {
                roots.push(path);
            }
        }
    }

    roots
}

fn installed_name_tokens(programs: &[ProgramEntry]) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();

    for program in programs {
        tokens.extend(name_tokens(&program.display_name));
        tokens.extend(name_tokens(&program.publisher));
    }

    tokens
}

fn folder_matches_installed(folder: &str, installed_tokens: &BTreeSet<String>) -> bool {
    name_tokens(folder)
        .iter()
        .any(|token| installed_tokens.contains(token))
}

fn name_tokens(text: &str) -> BTreeSet<String> {
    const STOPWORDS: &[&str] = &[
        "the",
        "inc",
        "llc",
        "ltd",
        "corp",
        "corporation",
        "software",
        "technologies",
        "technology",
        "company",
        "limited",
        "gmbh",
        "app",
        "apps",
        "desktop",
        "client",
        "studio",
        "suite",
        "update",
        "setup",
        "installer",
        "program",
        "programs",
        "windows",
        "version",
        "service",
        "services",
        "system",
        "systems",
        "tool",
        "tools",
        "driver",
        "drivers",
        "edition",
        "professional",
        "premium",
        "free",
        "open",
        "source",
    ];

    text.split(|character: char| !character.is_alphanumeric())
        .filter_map(|token| {
            let token = token.to_lowercase();
            if token.len() >= 3 && !STOPWORDS.contains(&token.as_str()) {
                Some(token)
            } else {
                None
            }
        })
        .collect()
}

fn installed_references(programs: &[ProgramEntry]) -> Vec<PathBuf> {
    let mut references = BTreeSet::new();

    for program in programs {
        if let Some(location) = &program.install_location {
            let normalized = normalize_path(location);
            if is_absolute_reference(&normalized) {
                references.insert(normalized);
            }
        }
        if let Some(ProgramCommand::Command { executable, .. }) = &program.command
            && let Some(parent) = Path::new(executable).parent()
        {
            let normalized = normalize_path(parent);
            if is_absolute_reference(&normalized) {
                references.insert(normalized);
            }
        }
    }

    references.into_iter().collect()
}

fn is_absolute_reference(path: &Path) -> bool {
    // Empty, relative and bare drive-root references would match every path
    // through starts_with and silently disable orphan detection.
    path.has_root() && path.components().count() >= 3
}

fn is_referenced(path: &Path, references: &[PathBuf]) -> bool {
    let path = normalize_path(path);

    references
        .iter()
        .any(|reference| path.starts_with(reference) || reference.starts_with(&path))
}

fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

fn program_files_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    for variable in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(path) = std::env::var_os(variable) {
            let path = PathBuf::from(path);
            if path.is_dir()
                && !roots
                    .iter()
                    .any(|root| normalize_path(root) == normalize_path(&path))
            {
                roots.push(path);
            }
        }
    }

    roots
}

fn find_uninstaller(folder: &Path) -> Option<PathBuf> {
    for entry in WalkDir::new(folder)
        .follow_links(false)
        .max_depth(2)
        .into_iter()
        .filter_entry(should_visit)
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with("unins") && name.ends_with(".exe") {
            return Some(entry.path().to_path_buf());
        }
    }

    None
}

fn is_system_folder(name: &str) -> bool {
    matches!(
        name,
        "common files"
            | "internet explorer"
            | "windowsapps"
            | "windows defender"
            | "windows defender advanced threat protection"
            | "windows mail"
            | "windows media player"
            | "windows nt"
            | "windows sidebar"
            | "windowspowershell"
            | "microsoft"
            | "msbuild"
            | "reference assemblies"
            | "dotnet"
            | "uninstall information"
            | "modifiablewindowsapps"
            | "packages"
            | "temp"
            | "virtualstore"
            | "connecteddevicesplatform"
            | "comms"
            | "placeholdertilelogo"
            | "d3dscache"
            | "nvidia"
            | "nvidia corporation"
            | "amd"
            | "intel"
            | "package cache"
            | "usoprivate"
            | "usoshared"
            | "regid.1991-06.com.microsoft"
            | "chipclean"
    )
}

fn should_visit(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if entry.file_type().is_symlink() {
        return false;
    }
    if !entry.file_type().is_dir() {
        return true;
    }

    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    if let Ok(metadata) = std::fs::symlink_metadata(entry.path())
        && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return false;
    }

    true
}

fn root_exists(path: &Path) -> bool {
    path.ancestors()
        .last()
        .map(|root| root.exists())
        .unwrap_or(false)
}

fn is_windows_apps(path: &Path) -> bool {
    path.to_string_lossy()
        .to_lowercase()
        .contains(r"\windowsapps\")
}

fn expand_env(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];

        if let Some(end) = after.find('%') {
            let name = &after[..end];
            if name.is_empty() {
                output.push_str("%%");
            } else if let Some(value) = std::env::var_os(name) {
                output.push_str(&value.to_string_lossy());
            } else {
                output.push('%');
                output.push_str(name);
                output.push('%');
            }
            rest = &after[end + 1..];
        } else {
            output.push('%');
            output.push_str(after);
            rest = "";
        }
    }

    output.push_str(rest);
    output
}

#[derive(Debug, Default)]
pub struct DeleteOutcome {
    pub deleted: usize,
    pub errors: Vec<String>,
}

pub fn delete_entries(entries: &[TraceEntry]) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();

    for entry in entries {
        if !entry.path.exists() {
            continue;
        }

        match delete_path(&entry.path) {
            Ok(()) => {
                outcome.deleted += 1;
                journal(json!({
                    "ts": now(),
                    "event": "trace_deleted_permanently",
                    "category": entry.category.key(),
                    "path": entry.path.display().to_string(),
                }));
            }
            Err(err) => {
                outcome
                    .errors
                    .push(format!("{}: {err}", entry.path.display()));
                journal(json!({
                    "ts": now(),
                    "event": "trace_delete_failed",
                    "category": entry.category.key(),
                    "path": entry.path.display().to_string(),
                    "error": err.to_string(),
                }));
            }
        }
    }

    outcome
}

fn delete_path(path: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let reparse = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;

    if metadata.is_dir() && !reparse {
        fs::remove_dir_all(path)
            .with_context(|| format!("не удалось удалить папку {}", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("не удалось удалить {}", path.display()))?;
    }

    Ok(())
}

pub fn quarantine_entries(entries: &[TraceEntry]) -> Result<usize> {
    quarantine_entries_with_paths(
        entries,
        &config::traces_store_path(),
        &config::traces_quarantine_dir(),
    )
}

fn quarantine_entries_with_paths(
    entries: &[TraceEntry],
    store_path: &Path,
    quarantine_dir: &Path,
) -> Result<usize> {
    fs::create_dir_all(quarantine_dir)
        .with_context(|| format!("не удалось создать {}", quarantine_dir.display()))?;

    let mut store = load_store(store_path)?;
    let mut moved: Vec<(PathBuf, PathBuf, TraceCategoryId)> = Vec::new();

    for entry in entries {
        if !entry.path.exists() {
            continue;
        }

        let stored = unique_quarantine_path(quarantine_dir, &entry.path);
        if let Err(err) = fs_move::move_path(&entry.path, &stored) {
            rollback_moves(&moved);
            return Err(err)
                .with_context(|| format!("не удалось переместить {}", entry.path.display()));
        }

        moved.push((entry.path.clone(), stored.clone(), entry.category));
        store.entries.push(QuarantinedTrace {
            category: entry.category,
            title: entry.title.clone(),
            detail: entry.detail.clone(),
            original_path: entry.path.clone(),
            stored_path: stored,
            quarantined_at: now(),
        });
    }

    if let Err(err) = save_store(store_path, &store) {
        rollback_moves(&moved);
        return Err(err);
    }

    for (original, stored, category) in &moved {
        journal(json!({
            "ts": now(),
            "event": "trace_quarantined",
            "category": category.key(),
            "original": original.display().to_string(),
            "quarantine": stored.display().to_string(),
        }));
    }

    Ok(moved.len())
}

pub fn restore_quarantined(entry: &QuarantinedTrace) -> Result<()> {
    let roots = trace_original_roots(entry.category);
    let direct_child = matches!(
        entry.category,
        TraceCategoryId::OrphanedFolders | TraceCategoryId::DeepLeftovers
    );
    restore_quarantined_with_paths(
        entry,
        &config::traces_store_path(),
        &config::traces_quarantine_dir(),
        &roots,
        direct_child,
    )
}

fn restore_quarantined_with_paths(
    entry: &QuarantinedTrace,
    store_path: &Path,
    quarantine_dir: &Path,
    allowed_roots: &[PathBuf],
    direct_child: bool,
) -> Result<()> {
    if entry.original_path.exists() {
        bail!(
            "Исходный путь уже существует: {}",
            entry.original_path.display()
        );
    }
    if !entry.stored_path.exists() {
        bail!(
            "Объект карантина не найден: {}",
            entry.stored_path.display()
        );
    }
    path_guard::ensure_not_reparse(&entry.stored_path)?;
    let quarantine_roots = [quarantine_dir.to_path_buf()];
    path_guard::ensure_existing_within(&entry.stored_path, &quarantine_roots)?;

    let destination = path_guard::canonical_destination(&entry.original_path)?;
    if direct_child {
        path_guard::ensure_direct_child(&destination, allowed_roots)?;
    } else {
        path_guard::ensure_within(&destination, allowed_roots)?;
    }

    fs_move::move_path(&entry.stored_path, &destination).with_context(|| {
        format!(
            "не удалось вернуть {} на место {}",
            entry.stored_path.display(),
            destination.display()
        )
    })?;

    let mut store = load_store(store_path)?;
    store.entries.retain(|item| item != entry);
    if let Err(err) = save_store(store_path, &store) {
        let _ = fs_move::move_path(&destination, &entry.stored_path);
        return Err(err);
    }

    journal(json!({
        "ts": now(),
        "event": "trace_restored",
        "original": destination.display().to_string(),
    }));

    Ok(())
}

pub fn load_quarantine() -> Result<TraceStore> {
    load_store(&config::traces_store_path())
}

pub fn clear_quarantine_metadata() -> Result<()> {
    let path = config::traces_store_path();
    if path.exists() {
        save_store(&path, &TraceStore::default())?;
    }
    Ok(())
}
fn load_store(path: &Path) -> Result<TraceStore> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(TraceStore::default()),
        Err(err) => {
            return Err(err).with_context(|| format!("не удалось прочитать {}", path.display()));
        }
    };

    serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("не удалось разобрать {}", path.display()))
}

fn save_store(path: &Path, store: &TraceStore) -> Result<()> {
    fs_atomic::write_json_atomic(path, store)
}

fn unique_quarantine_path(dir: &Path, original: &Path) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let name = original
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "trace".to_owned());
    let mut candidate = dir.join(format!("{stamp}-{name}"));
    let mut counter = 1_u32;

    while candidate.exists() {
        candidate = dir.join(format!("{stamp}-{counter}-{name}"));
        counter += 1;
    }

    candidate
}

fn rollback_moves(moved: &[(PathBuf, PathBuf, TraceCategoryId)]) {
    for (original, stored, _) in moved.iter().rev() {
        let _ = fs_move::move_path(stored, original);
    }
}

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write trace journal event");
            } else {
                let _ = journal.flush();
            }
        }
        Err(err) => tracing::warn!(%err, "failed to open operations journal"),
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traces.json");
        let store = TraceStore {
            entries: vec![QuarantinedTrace {
                category: TraceCategoryId::BrokenShortcuts,
                title: "test.lnk".to_owned(),
                detail: "missing".to_owned(),
                original_path: PathBuf::from(r"C:\Desktop\test.lnk"),
                stored_path: PathBuf::from(r"C:\Quarantine\test.lnk"),
                quarantined_at: now(),
            }],
        };

        save_store(&path, &store).unwrap();
        let loaded = load_store(&path).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].title, "test.lnk");
    }

    #[test]
    fn quarantines_and_restores_a_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("broken.lnk");
        fs::write(&original, b"shortcut").unwrap();

        let store_path = dir.path().join("store.json");
        let quarantine_dir = dir.path().join("quarantine");
        let entry = TraceEntry {
            category: TraceCategoryId::BrokenShortcuts,
            title: "broken.lnk".to_owned(),
            detail: "missing".to_owned(),
            path: original.clone(),
            kind: TraceKind::Shortcut {
                target: PathBuf::from(r"C:\Missing\app.exe"),
            },
        };

        assert_eq!(
            quarantine_entries_with_paths(&[entry], &store_path, &quarantine_dir,).unwrap(),
            1
        );
        assert!(!original.exists());
        let store = load_store(&store_path).unwrap();
        assert_eq!(store.entries.len(), 1);

        let roots = vec![dir.path().to_path_buf()];
        restore_quarantined_with_paths(
            &store.entries[0],
            &store_path,
            &quarantine_dir,
            &roots,
            false,
        )
        .unwrap();
        assert!(original.exists());
        assert!(load_store(&store_path).unwrap().entries.is_empty());
    }

    #[test]
    fn permanently_deletes_file_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("broken.lnk");
        fs::write(&file, b"shortcut").unwrap();
        let folder = dir.path().join("OrphanApp");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("unins000.exe"), b"stub").unwrap();

        let entries = vec![
            TraceEntry {
                category: TraceCategoryId::BrokenShortcuts,
                title: "broken.lnk".to_owned(),
                detail: "missing".to_owned(),
                path: file.clone(),
                kind: TraceKind::Shortcut {
                    target: PathBuf::from(r"C:\Missing\app.exe"),
                },
            },
            TraceEntry {
                category: TraceCategoryId::OrphanedFolders,
                title: "OrphanApp".to_owned(),
                detail: "orphan".to_owned(),
                path: folder.clone(),
                kind: TraceKind::OrphanFolder {
                    evidence: folder.join("unins000.exe"),
                },
            },
        ];

        let outcome = delete_entries(&entries);
        assert_eq!(outcome.deleted, 2);
        assert!(outcome.errors.is_empty());
        assert!(!file.exists());
        assert!(!folder.exists());
    }

    #[test]
    fn quarantines_and_restores_a_temporary_folder() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("OrphanApp");
        fs::create_dir_all(&original).unwrap();
        fs::write(original.join("unins000.exe"), b"stub").unwrap();

        let store_path = dir.path().join("store.json");
        let quarantine_dir = dir.path().join("quarantine");
        let entry = TraceEntry {
            category: TraceCategoryId::OrphanedFolders,
            title: "OrphanApp".to_owned(),
            detail: "orphan".to_owned(),
            path: original.clone(),
            kind: TraceKind::OrphanFolder {
                evidence: original.join("unins000.exe"),
            },
        };

        quarantine_entries_with_paths(&[entry], &store_path, &quarantine_dir).unwrap();
        assert!(!original.exists());
        let store = load_store(&store_path).unwrap();
        let roots = vec![dir.path().to_path_buf()];
        restore_quarantined_with_paths(
            &store.entries[0],
            &store_path,
            &quarantine_dir,
            &roots,
            true,
        )
        .unwrap();
        assert!(original.exists());
        assert!(original.join("unins000.exe").exists());
    }

    #[test]
    fn refuses_to_restore_from_outside_quarantine() {
        let dir = tempfile::tempdir().unwrap();
        let quarantine = dir.path().join("quarantine");
        let allowed = dir.path().join("Allowed");
        fs::create_dir_all(&quarantine).unwrap();
        fs::create_dir_all(&allowed).unwrap();

        let stored = dir.path().join("outside.lnk");
        fs::write(&stored, b"shortcut").unwrap();
        let original = allowed.join("restored.lnk");
        let store_path = dir.path().join("store.json");
        let entry = QuarantinedTrace {
            category: TraceCategoryId::BrokenShortcuts,
            title: "restored.lnk".to_owned(),
            detail: "test".to_owned(),
            original_path: original.clone(),
            stored_path: stored.clone(),
            quarantined_at: now(),
        };
        save_store(
            &store_path,
            &TraceStore {
                entries: vec![entry.clone()],
            },
        )
        .unwrap();

        let result = restore_quarantined_with_paths(
            &entry,
            &store_path,
            &quarantine,
            std::slice::from_ref(&allowed),
            false,
        );
        assert!(result.is_err());
        assert!(stored.exists());
        assert!(!original.exists());
    }

    #[test]
    fn refuses_to_restore_outside_allowed_roots() {
        let dir = tempfile::tempdir().unwrap();
        let quarantine = dir.path().join("quarantine");
        let allowed = dir.path().join("Allowed");
        let outside = dir.path().join("Outside");
        fs::create_dir_all(&quarantine).unwrap();
        fs::create_dir_all(&allowed).unwrap();
        fs::create_dir_all(&outside).unwrap();

        let stored = quarantine.join("item.lnk");
        fs::write(&stored, b"shortcut").unwrap();
        let original = outside.join("restored.lnk");
        let store_path = dir.path().join("store.json");
        let entry = QuarantinedTrace {
            category: TraceCategoryId::BrokenShortcuts,
            title: "restored.lnk".to_owned(),
            detail: "test".to_owned(),
            original_path: original.clone(),
            stored_path: stored.clone(),
            quarantined_at: now(),
        };
        save_store(
            &store_path,
            &TraceStore {
                entries: vec![entry.clone()],
            },
        )
        .unwrap();

        let result = restore_quarantined_with_paths(
            &entry,
            &store_path,
            &quarantine,
            std::slice::from_ref(&allowed),
            false,
        );
        assert!(result.is_err());
        assert!(stored.exists());
        assert!(!original.exists());
    }

    #[test]
    fn matches_installed_name_tokens() {
        let mut tokens = BTreeSet::new();
        tokens.extend(name_tokens("Discord Inc."));
        tokens.extend(name_tokens("Mozilla"));

        assert!(folder_matches_installed("Discord", &tokens));
        assert!(folder_matches_installed("Mozilla", &tokens));
        assert!(!folder_matches_installed("SomeOrphanApp", &tokens));
    }

    #[test]
    fn detects_referenced_paths() {
        let references = vec![normalize_path(Path::new(r"C:\Program Files\Vendor\App"))];
        assert!(is_referenced(
            Path::new(r"C:\Program Files\Vendor\App\bin"),
            &references
        ));
        assert!(is_referenced(
            Path::new(r"C:\Program Files\Vendor"),
            &references
        ));
        assert!(!is_referenced(
            Path::new(r"C:\Program Files\Vendor\Other"),
            &references
        ));
    }

    #[test]
    fn rejects_empty_relative_and_root_references() {
        assert!(!is_absolute_reference(Path::new("")));
        assert!(!is_absolute_reference(Path::new("RUNDLL32.EXE")));
        assert!(!is_absolute_reference(Path::new(r"C:\")));
        assert!(!is_absolute_reference(Path::new(r"C:")));
        assert!(is_absolute_reference(Path::new(r"C:\Program Files\App")));
    }
}
