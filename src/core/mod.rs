pub mod catalog;
pub mod cleaner;
pub mod community;
pub mod community_state;
pub mod fs_atomic;
pub mod fs_move;
pub mod job;
pub mod journal;
pub mod model;
pub mod path_guard;
pub mod privacy;
pub mod privacy_state;
pub mod quarantine;
pub mod reg_file;
pub mod registry;
pub mod registry_editor;
pub mod registry_editor_state;
pub mod registry_import;
pub mod registry_snapshots;
pub mod registry_snapshots_state;
pub mod registry_state;
pub mod rollback;
pub mod rollback_state;
pub mod safety;
pub mod scanner;
pub mod services;
pub mod startup;
pub mod startup_state;
pub mod state;
pub mod tasks;
pub mod traces;
pub mod traces_state;
pub mod uninstall;
pub mod uninstall_state;

/// Tests that create keys or call reg.exe export must not run concurrently:
/// parallel backups occasionally fail with access denied on a busy hive.
#[cfg(test)]
pub(crate) fn registry_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Millisecond timestamp plus a process-wide counter so ids and file names
/// created within the same millisecond do not collide.
pub(crate) fn unique_stamp() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f"),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}
