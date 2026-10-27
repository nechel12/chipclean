use std::{mem::ManuallyDrop, sync::Once};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde_json::json;
use windows::{
    Win32::{
        Foundation::{VARIANT_FALSE, VARIANT_TRUE},
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            TaskScheduler::{
                IExecAction, ITaskFolder, ITaskService, TASK_ACTION_EXEC, TASK_ACTION_TYPE,
                TASK_ENUM_HIDDEN, TASK_STATE, TASK_STATE_DISABLED, TASK_STATE_QUEUED,
                TASK_STATE_READY, TASK_STATE_RUNNING, TaskScheduler,
            },
            Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4},
        },
    },
    core::{BSTR, Interface},
};

use crate::core::journal::Journal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEntry {
    pub title: String,
    pub path: String,
    pub command: String,
    pub status: String,
    pub enabled: bool,
    pub system: bool,
}

static COM_INIT: Once = Once::new();

pub fn scan_tasks() -> Result<Vec<TaskEntry>> {
    init_com();
    let service = connect()?;
    let root = unsafe { service.GetFolder(&BSTR::from("\\")) }
        .context("не удалось открыть корневую папку планировщика")?;
    let mut entries = Vec::new();
    collect_folder(&root, &mut entries, 0);
    entries.sort_by(|left, right| {
        left.system
            .cmp(&right.system)
            .then_with(|| right.enabled.cmp(&left.enabled))
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });
    Ok(entries)
}

pub fn set_task_enabled(path: &str, enabled: bool) -> Result<String> {
    if path.trim().is_empty() {
        bail!("Путь задачи пуст");
    }

    init_com();
    let service = connect()?;
    let root = unsafe { service.GetFolder(&BSTR::from("\\")) }
        .context("не удалось открыть корневую папку планировщика")?;
    let task = unsafe { root.GetTask(&BSTR::from(path)) }
        .with_context(|| format!("не удалось открыть задачу {path}"))?;
    let current = unsafe { task.Enabled() }
        .context("не удалось прочитать состояние задачи")?
        .0
        != 0;
    if current == enabled {
        bail!("Состояние задачи не изменилось; обновите список.");
    }

    unsafe { task.SetEnabled(if enabled { VARIANT_TRUE } else { VARIANT_FALSE }) }
        .with_context(|| format!("не удалось изменить состояние задачи {path}"))?;

    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": if enabled { "task_enabled" } else { "task_disabled" },
        "path": path,
    }));

    Ok(if enabled {
        "Задача включена.".to_owned()
    } else {
        "Задача отключена. Уже запущенный экземпляр не останавливается.".to_owned()
    })
}

fn connect() -> Result<ITaskService> {
    let service: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
            .context("не удалось создать Task Scheduler COM-объект")?;
    let empty = VARIANT::default();
    unsafe { service.Connect(&empty, &empty, &empty, &empty) }
        .context("не удалось подключиться к планировщику задач")?;
    Ok(service)
}

fn init_com() {
    COM_INIT.call_once(|| {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    });
}

fn collect_folder(folder: &ITaskFolder, entries: &mut Vec<TaskEntry>, depth: u32) {
    if depth > 8 {
        return;
    }

    if let Ok(tasks) = unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) } {
        let count = unsafe { tasks.Count() }.unwrap_or(0);
        for index in 0..count {
            let Ok(task) = (unsafe { tasks.get_Item(&index_variant(index)) }) else {
                continue;
            };
            let path = unsafe { task.Path() }
                .map(|value| value.to_string())
                .unwrap_or_default();
            if path.is_empty() {
                continue;
            }
            let name = unsafe { task.Name() }
                .map(|value| value.to_string())
                .unwrap_or_default();
            let enabled = unsafe { task.Enabled() }
                .map(|value| value.0 != 0)
                .unwrap_or(false);
            let status = unsafe { task.State() }
                .map(task_state_label)
                .unwrap_or("Неизвестно");
            let command = task_command(&task);

            entries.push(TaskEntry {
                title: task_title(&path, &name),
                system: is_windows_task(&path),
                path,
                command,
                status: status.to_owned(),
                enabled,
            });
        }
    }

    if let Ok(folders) = unsafe { folder.GetFolders(0) } {
        let count = unsafe { folders.Count() }.unwrap_or(0);
        for index in 0..count {
            let Ok(child) = (unsafe { folders.get_Item(&index_variant(index)) }) else {
                continue;
            };
            collect_folder(&child, entries, depth + 1);
        }
    }
}

fn task_command(task: &windows::Win32::System::TaskScheduler::IRegisteredTask) -> String {
    let Ok(definition) = (unsafe { task.Definition() }) else {
        return String::new();
    };
    let Ok(actions) = (unsafe { definition.Actions() }) else {
        return String::new();
    };
    let mut count = 0;
    if unsafe { actions.Count(&mut count) }.is_err() || count < 1 {
        return String::new();
    }
    let Ok(action) = (unsafe { actions.get_Item(1) }) else {
        return String::new();
    };
    let mut action_type = TASK_ACTION_TYPE::default();
    if unsafe { action.Type(&mut action_type) }.is_err() || action_type != TASK_ACTION_EXEC {
        return String::new();
    }
    let Ok(exec) = action.cast::<IExecAction>() else {
        return String::new();
    };

    let mut path = BSTR::new();
    let mut arguments = BSTR::new();
    let _ = unsafe { exec.Path(&mut path) };
    let _ = unsafe { exec.Arguments(&mut arguments) };
    let path = path.to_string();
    let arguments = arguments.to_string();
    if arguments.trim().is_empty() {
        path
    } else {
        format!("{path} {arguments}")
    }
}

fn task_state_label(state: TASK_STATE) -> &'static str {
    if state == TASK_STATE_DISABLED {
        "Отключена"
    } else if state == TASK_STATE_QUEUED {
        "В очереди"
    } else if state == TASK_STATE_READY {
        "Готова"
    } else if state == TASK_STATE_RUNNING {
        "Выполняется"
    } else {
        "Неизвестно"
    }
}

fn task_title(path: &str, name: &str) -> String {
    if !name.trim().is_empty() {
        return name.to_owned();
    }
    path.rsplit('\\')
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}

fn is_windows_task(path: &str) -> bool {
    path.to_ascii_lowercase().starts_with("\\microsoft\\")
}

fn index_variant(index: i32) -> VARIANT {
    let value = VARIANT_0_0 {
        vt: VT_I4,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: VARIANT_0_0_0 { lVal: index },
    };

    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(value),
        },
    }
}

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write task journal event");
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
    fn detects_windows_tasks() {
        assert!(is_windows_task(
            r"\Microsoft\Windows\Defrag\ScheduledDefrag"
        ));
        assert!(!is_windows_task(r"\MyVendor\Updater"));
    }

    #[test]
    fn falls_back_to_path_title() {
        assert_eq!(task_title(r"\MyVendor\Updater", ""), "Updater");
        assert_eq!(
            task_title(r"\MyVendor\Updater", "Vendor Update"),
            "Vendor Update"
        );
    }
}
