use std::{path::PathBuf, process::Command};

use anyhow::{Context, Result, bail};
use serde_json::json;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::UI::Shell::CommandLineToArgvW;
use windows::core::PCWSTR;

use crate::core::{journal::Journal, registry::RegistryHive};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramSource {
    CurrentUser,
    LocalMachine,
    LocalMachine32,
}

impl ProgramSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::CurrentUser => "HKCU",
            Self::LocalMachine => "HKLM",
            Self::LocalMachine32 => "HKLM 32-bit",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProgramCommand {
    Msi {
        product_code: String,
    },
    Command {
        executable: String,
        arguments: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ProgramEntry {
    pub display_name: String,
    pub version: String,
    pub publisher: String,
    pub size_kb: u32,
    pub source: ProgramSource,
    pub install_location: Option<PathBuf>,
    pub command: Option<ProgramCommand>,
    pub command_display: String,
    pub quiet: bool,
    pub key_display: String,
}

const PROGRAM_KEYS: &[(ProgramSource, RegistryHive, &str)] = &[
    (
        ProgramSource::CurrentUser,
        RegistryHive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
    (
        ProgramSource::LocalMachine,
        RegistryHive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
    (
        ProgramSource::LocalMachine32,
        RegistryHive::LocalMachine,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
];

pub fn scan_programs() -> Result<Vec<ProgramEntry>> {
    let mut entries = Vec::new();

    for (source, hive, key_path) in PROGRAM_KEYS {
        let Ok(parent) = hive.root().open(*key_path) else {
            continue;
        };
        let Ok(keys) = parent.keys() else {
            continue;
        };

        for subkey_name in keys {
            let Ok(subkey) = parent.open(&subkey_name) else {
                continue;
            };

            let Ok(display_name) = subkey.get_string("DisplayName") else {
                continue;
            };
            let display_name = display_name.trim().to_owned();
            if display_name.is_empty() {
                continue;
            }
            if subkey.get_u32("SystemComponent").unwrap_or(0) == 1 {
                continue;
            }
            if subkey.get_string("ParentKeyName").is_ok() {
                continue;
            }
            if subkey.get_u32("NoRemove").unwrap_or(0) == 1 {
                continue;
            }
            if let Ok(release_type) = subkey.get_string("ReleaseType") {
                let release_type = release_type.to_ascii_lowercase();
                if release_type.contains("update")
                    || release_type.contains("hotfix")
                    || release_type.contains("security")
                {
                    continue;
                }
            }

            let version = subkey.get_string("DisplayVersion").unwrap_or_default();
            let publisher = subkey
                .get_string("Publisher")
                .ok()
                .filter(|publisher| !publisher.trim().is_empty())
                .unwrap_or_else(|| "Издатель не указан".to_owned());
            let size_kb = subkey.get_u32("EstimatedSize").unwrap_or(0);
            let install_location = subkey
                .get_string("InstallLocation")
                .ok()
                .map(|path| path.trim().to_owned())
                .filter(|path| !path.is_empty())
                .map(PathBuf::from);

            let windows_installer = subkey.get_u32("WindowsInstaller").unwrap_or(0) == 1;
            let product_code = subkey
                .get_string("ProductCode")
                .ok()
                .filter(|code| !code.trim().is_empty());
            let uninstall = subkey
                .get_string("UninstallString")
                .ok()
                .filter(|command| !command.trim().is_empty());
            let quiet_uninstall = subkey
                .get_string("QuietUninstallString")
                .ok()
                .filter(|command| !command.trim().is_empty());

            let Some((command, command_display, quiet)) = build_program_command(
                windows_installer,
                product_code.as_deref(),
                uninstall.as_deref(),
                quiet_uninstall.as_deref(),
            ) else {
                entries.push(ProgramEntry {
                    display_name,
                    version,
                    publisher,
                    size_kb,
                    source: *source,
                    install_location,
                    command: None,
                    command_display: "Штатный деинсталлятор не найден".to_owned(),
                    quiet: false,
                    key_display: format!("{}\\{}\\{}", source.label(), key_path, subkey_name),
                });
                continue;
            };

            entries.push(ProgramEntry {
                display_name,
                version,
                publisher,
                size_kb,
                source: *source,
                install_location,
                command: Some(command),
                command_display,
                quiet,
                key_display: format!("{}\\{}\\{}", source.label(), key_path, subkey_name),
            });
        }
    }

    entries.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
    });

    Ok(entries)
}

pub fn launch_program(entry: &ProgramEntry) -> Result<String> {
    let Some(command) = &entry.command else {
        bail!("Штатный деинсталлятор не найден.");
    };

    match command {
        ProgramCommand::Msi { product_code } => {
            Command::new("msiexec.exe")
                .arg("/X")
                .arg(product_code)
                .spawn()
                .context("не удалось запустить msiexec.exe")?;
        }
        ProgramCommand::Command {
            executable,
            arguments,
        } => {
            let lower = executable.to_ascii_lowercase();
            if lower.ends_with(".bat") || lower.ends_with(".cmd") {
                Command::new("cmd.exe")
                    .arg("/C")
                    .arg(executable)
                    .args(arguments)
                    .spawn()
                    .context("не удалось запустить командный деинсталлятор")?;
            } else {
                Command::new(executable)
                    .args(arguments)
                    .spawn()
                    .with_context(|| format!("не удалось запустить {executable}"))?;
            }
        }
    }

    journal(json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "event": "program_uninstaller_started",
        "program": entry.display_name,
        "publisher": entry.publisher,
        "source": entry.source.label(),
        "command": entry.command_display,
        "quiet": entry.quiet,
    }));

    Ok(format!("Деинсталлятор запущен: {}", entry.display_name))
}

fn build_program_command(
    windows_installer: bool,
    product_code: Option<&str>,
    uninstall: Option<&str>,
    quiet_uninstall: Option<&str>,
) -> Option<(ProgramCommand, String, bool)> {
    if windows_installer {
        if let Some(code) = product_code {
            return Some((
                ProgramCommand::Msi {
                    product_code: code.to_owned(),
                },
                format!("msiexec.exe /X {code}"),
                false,
            ));
        }

        if let Some(command) = uninstall
            && let Some(code) = extract_msi_product_code(command)
        {
            return Some((
                ProgramCommand::Msi {
                    product_code: code.clone(),
                },
                format!("msiexec.exe /X {code}"),
                false,
            ));
        }
    }

    if let Some(command) = uninstall
        && command.to_ascii_lowercase().contains("msiexec")
        && let Some(code) = extract_msi_product_code(command)
    {
        return Some((
            ProgramCommand::Msi {
                product_code: code.clone(),
            },
            format!("msiexec.exe /X {code}"),
            false,
        ));
    }

    if let Some(command) = uninstall
        && let Some(parsed) = parse_command(command)
    {
        return Some((parsed.0, parsed.1, false));
    }

    if let Some(command) = quiet_uninstall
        && let Some(parsed) = parse_command(command)
    {
        return Some((parsed.0, parsed.1, true));
    }

    None
}

fn parse_command(command: &str) -> Option<(ProgramCommand, String)> {
    let expanded = expand_env(command);
    let (executable, arguments) = split_command_line(&expanded).ok()?;
    let display = if arguments.is_empty() {
        executable.clone()
    } else {
        format!("{} {}", executable, arguments.join(" "))
    };

    Some((
        ProgramCommand::Command {
            executable,
            arguments,
        },
        display,
    ))
}

fn split_command_line(command: &str) -> Result<(String, Vec<String>)> {
    let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
    let mut argc = 0_i32;
    let argv = unsafe { CommandLineToArgvW(PCWSTR(wide.as_ptr()), &mut argc) };
    if argv.is_null() {
        bail!("не удалось разобрать командную строку");
    }

    let result = (|| {
        if argc <= 0 {
            bail!("пустая командная строка");
        }

        let mut args = Vec::with_capacity(argc as usize);
        for index in 0..argc as usize {
            let argument = unsafe { *argv.add(index) };
            args.push(unsafe { argument.to_string() }.unwrap_or_default());
        }

        let executable = args.remove(0);
        Ok((executable, args))
    })();

    unsafe {
        LocalFree(Some(HLOCAL(argv as *mut core::ffi::c_void)));
    }

    result
}

fn extract_msi_product_code(command: &str) -> Option<String> {
    let upper = command.to_ascii_uppercase();
    for prefix in ["/I", "/X", "-I", "-X"] {
        let Some(position) = upper.find(prefix) else {
            continue;
        };
        let rest = &command[position + prefix.len()..];
        if let Some(start) = rest.find('{')
            && let Some(end) = rest[start..].find('}')
        {
            return Some(rest[start..=start + end].to_owned());
        }
    }

    None
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

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write uninstall journal event");
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
    fn splits_quoted_command() {
        let (executable, arguments) =
            split_command_line(r#""C:\Program Files\App\uninstall.exe" /S /remove"#).unwrap();
        assert_eq!(executable, r"C:\Program Files\App\uninstall.exe");
        assert_eq!(arguments, vec!["/S", "/remove"]);
    }

    #[test]
    fn builds_msi_uninstall_command() {
        let (command, display, quiet) = build_program_command(
            true,
            Some("{11111111-2222-3333-4444-555555555555}"),
            Some(r#"MsiExec.exe /I{11111111-2222-3333-4444-555555555555}"#),
            None,
        )
        .unwrap();

        assert!(matches!(command, ProgramCommand::Msi { .. }));
        assert!(display.starts_with("msiexec.exe /X"));
        assert!(!quiet);
    }

    #[test]
    fn builds_msi_from_uninstall_string_without_flag() {
        let (command, display, quiet) =
            build_program_command(false, None, Some(r#"MsiExec.exe /I{ABC}"#), None).unwrap();
        assert!(matches!(command, ProgramCommand::Msi { .. }));
        assert!(display.starts_with("msiexec.exe /X"));
        assert!(!quiet);
    }

    #[test]
    fn extracts_product_code_from_msi_string() {
        assert_eq!(
            extract_msi_product_code(r#"MsiExec.exe /I{ABC}"#),
            Some("{ABC}".to_owned())
        );
        assert_eq!(extract_msi_product_code(r#"C:\App\uninstall.exe"#), None);
    }

    #[test]
    fn expands_environment_variables() {
        let expanded = expand_env(r"%WINDIR%\System32\notepad.exe");
        assert!(!expanded.contains('%'));
        assert!(expanded.to_lowercase().ends_with(r"\system32\notepad.exe"));
    }
}
