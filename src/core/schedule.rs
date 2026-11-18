//! Фоновая очистка по расписанию через Планировщик заданий Windows.
//!
//! Задачи регистрируются через COM (ITaskService) — без обращения к внешней
//! утилите `schtasks.exe`. Регистрируются две задачи:
//!
//! * `ChipClean Background Cleanup` — ежедневно в заданное время;
//! * `ChipClean Startup Check` — при входе в систему.
//!
//! Обе запускают текущий исполняемый файл с аргументом `--background-cleanup`.

use std::cell::Cell;

use anyhow::{Context, Result};
use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            TaskScheduler::{
                IExecAction, IDailyTrigger, ILogonTrigger, ITaskDefinition, ITaskFolder,
                ITaskService, TASK_ACTION_EXEC, TASK_CREATE_OR_UPDATE,
                TASK_LOGON_INTERACTIVE_TOKEN, TASK_RUNLEVEL_HIGHEST, TASK_TRIGGER_DAILY,
                TASK_TRIGGER_LOGON, TaskScheduler,
            },
            Variant::VARIANT,
        },
    },
    core::{BSTR, Interface},
};

pub const TASK_NAME: &str = "ChipClean Background Cleanup";
pub const TASK_STARTUP_NAME: &str = "ChipClean Startup Check";

thread_local! {
    static COM_INIT: Cell<bool> = const { Cell::new(false) };
}

fn init_com() {
    COM_INIT.with(|initialized| {
        if !initialized.get() {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            initialized.set(true);
        }
    });
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

fn root_folder(service: &ITaskService) -> Result<ITaskFolder> {
    unsafe { service.GetFolder(&BSTR::from("\\")) }.context("не удалось открыть корневую папку")
}

/// Проверяет, зарегистрирована ли задача.
pub fn is_registered() -> bool {
    task_exists(TASK_NAME)
}

/// Проверяет, зарегистрирована ли задача проверки при входе.
pub fn is_startup_check_registered() -> bool {
    task_exists(TASK_STARTUP_NAME)
}

fn task_exists(name: &str) -> bool {
    init_com();
    let Ok(service) = connect() else {
        return false;
    };
    let Ok(folder) = root_folder(&service) else {
        return false;
    };
    unsafe { folder.GetTask(&BSTR::from(name)) }.is_ok()
}

/// Создаёт или обновляет ежедневную задачу очистки.
pub fn register(hour: u32, minute: u32) -> Result<()> {
    init_com();
    let service = connect()?;
    let folder = root_folder(&service)?;
    let definition = new_definition(&service)?;

    let triggers = unsafe { definition.Triggers() }
        .context("не удалось получить коллекцию триггеров")?;
    let trigger = unsafe { triggers.Create(TASK_TRIGGER_DAILY) }
        .context("не удалось создать ежедневный триггер")?;
    let daily: IDailyTrigger = trigger.cast().context("неверный тип триггера")?;

    let hour = hour.min(23);
    let minute = minute.min(59);
    let boundary = format!("2026-01-01T{hour:02}:{minute:02}:00");
    unsafe { daily.SetStartBoundary(&BSTR::from(boundary)) }
        .context("не удалось задать время запуска")?;
    unsafe { daily.SetDaysInterval(1) }.context("не удалось задать интервал")?;

    set_action(&definition, "--background-cleanup")?;
    set_principal(&definition)?;

    register_definition(&folder, TASK_NAME, &definition)
}

/// Регистрирует задачу проверки при входе в систему.
pub fn register_startup_check() -> Result<()> {
    init_com();
    let service = connect()?;
    let folder = root_folder(&service)?;
    let definition = new_definition(&service)?;

    let triggers = unsafe { definition.Triggers() }
        .context("не удалось получить коллекцию триггеров")?;
    let _: ILogonTrigger = unsafe { triggers.Create(TASK_TRIGGER_LOGON) }
        .context("не удалось создать триггер входа")?
        .cast()
        .context("неверный тип триггера")?;

    set_action(&definition, "--background-cleanup")?;
    set_principal(&definition)?;

    register_definition(&folder, TASK_STARTUP_NAME, &definition)
}

fn new_definition(service: &ITaskService) -> Result<ITaskDefinition> {
    unsafe { service.NewTask(0) }.context("не удалось создать определение задачи")
}

fn set_action(definition: &ITaskDefinition, arguments: &str) -> Result<()> {
    let actions = unsafe { definition.Actions() }
        .context("не удалось получить коллекцию действий")?;
    let action = unsafe { actions.Create(TASK_ACTION_EXEC) }
        .context("не удалось создать действие")?;
    let exec: IExecAction = action.cast().context("неверный тип действия")?;

    let exe = std::env::current_exe().context("не удалось получить путь к исполняемому файлу")?;
    unsafe { exec.SetPath(&BSTR::from(exe.display().to_string())) }
        .context("не удалось задать путь к исполняемому файлу")?;
    unsafe { exec.SetArguments(&BSTR::from(arguments)) }
        .context("не удалось задать аргументы")?;
    Ok(())
}

fn set_principal(definition: &ITaskDefinition) -> Result<()> {
    let principal = unsafe { definition.Principal() }
        .context("не удалось получить учётную запись задачи")?;
    unsafe { principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN) }
        .context("не удалось задать тип входа")?;
    unsafe { principal.SetRunLevel(TASK_RUNLEVEL_HIGHEST) }
        .context("не удалось задать уровень прав")?;
    Ok(())
}

fn register_definition(folder: &ITaskFolder, name: &str, definition: &ITaskDefinition) -> Result<()> {
    let empty = VARIANT::default();
    unsafe {
        folder.RegisterTaskDefinition(
            &BSTR::from(name),
            definition,
            TASK_CREATE_OR_UPDATE.0,
            &empty,
            &empty,
            TASK_LOGON_INTERACTIVE_TOKEN,
            &empty,
        )
    }
    .with_context(|| format!("не удалось зарегистрировать задачу {name}"))?;
    Ok(())
}

/// Удаляет задачу очистки.
pub fn unregister() -> Result<()> {
    delete_task(TASK_NAME)
}

/// Удаляет задачу проверки при входе.
pub fn unregister_startup_check() -> Result<()> {
    delete_task(TASK_STARTUP_NAME)
}

fn delete_task(name: &str) -> Result<()> {
    init_com();
    if !task_exists(name) {
        return Ok(());
    }
    let service = connect()?;
    let folder = root_folder(&service)?;
    unsafe { folder.DeleteTask(&BSTR::from(name), 0) }
        .with_context(|| format!("не удалось удалить задачу {name}"))?;
    Ok(())
}

/// Синхронизирует задачу очистки с настройками.
pub fn apply_enabled(enabled: bool, hour: u32, minute: u32) -> Result<()> {
    if enabled {
        register(hour, minute)
    } else {
        unregister()
    }
}

/// Синхронизирует задачу проверки при входе с настройками.
pub fn apply_startup_check_enabled(enabled: bool) -> Result<()> {
    if enabled {
        register_startup_check()
    } else {
        unregister_startup_check()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_names_are_stable() {
        assert_eq!(TASK_NAME, "ChipClean Background Cleanup");
        assert_eq!(TASK_STARTUP_NAME, "ChipClean Startup Check");
    }
}
