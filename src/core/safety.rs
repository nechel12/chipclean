use std::{fs, path::Path};

use crate::core::registry_editor::EditorHive;

pub const PREVIEW_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Safe,
    Outside,
    Blocked,
}

pub fn classify_file(path: &Path) -> Risk {
    let risk = classify_file_str(&path.to_string_lossy());
    if risk == Risk::Blocked {
        return risk;
    }
    match fs::canonicalize(path) {
        Ok(canonical) => worst(risk, classify_file_str(&canonical.to_string_lossy())),
        Err(_) => risk,
    }
}

pub fn classify_file_pattern(pattern: &str) -> Risk {
    if has_parent_dir(pattern) {
        return Risk::Blocked;
    }
    let expanded = expand_env(pattern).replace('/', "\\");
    let wildcard = expanded.find(['*', '?']);
    let root = match wildcard {
        Some(index) => {
            let prefix = &expanded[..index];
            let trimmed = prefix.trim_end_matches('\\');
            if trimmed.is_empty() {
                return Risk::Outside;
            }
            if prefix.ends_with('\\') {
                trimmed.to_owned()
            } else {
                trimmed
                    .rsplit_once('\\')
                    .map(|(parent, _)| parent.to_owned())
                    .unwrap_or_default()
            }
        }
        None => expanded.trim_end_matches('\\').to_owned(),
    };
    if root.is_empty() {
        return Risk::Outside;
    }
    let risk = classify_file_str(&root);
    if risk == Risk::Blocked {
        return risk;
    }
    match fs::canonicalize(&root) {
        Ok(canonical) => worst(risk, classify_file_str(&canonical.to_string_lossy())),
        Err(_) => risk,
    }
}

pub fn classify_registry(hive: EditorHive, path: &str, remove_self: bool) -> Risk {
    let path = path.trim().trim_matches('\\').replace('/', "\\");
    if path.is_empty() || path.split('\\').any(|part| part == "..") {
        return Risk::Blocked;
    }
    let lower = path.to_lowercase();
    let first = lower.split('\\').next().unwrap_or_default();

    let blocked = match hive {
        EditorHive::Hklm => matches!(
            first,
            "system" | "security" | "sam" | "hardware" | "bcd00000000"
        ),
        EditorHive::Hkcu => matches!(
            first,
            "environment"
                | "control panel"
                | "keyboard layout"
                | "network"
                | "volatile environment"
        ),
        _ => true,
    };
    if blocked {
        return Risk::Blocked;
    }

    if hive == EditorHive::Hklm
        && [
            r"software\microsoft\windows nt\currentversion\profilelist",
            r"software\microsoft\windows nt\currentversion\winlogon",
        ]
        .iter()
        .any(|critical| lower == *critical || lower.starts_with(&format!("{critical}\\")))
    {
        return Risk::Blocked;
    }

    let safe = matches!(hive, EditorHive::Hkcu | EditorHive::Hklm)
        && (lower == "software" || lower.starts_with("software\\"));
    if !safe {
        return Risk::Outside;
    }
    if remove_self {
        const EXACT: &[&str] = &[
            "software",
            "software\\microsoft",
            "software\\classes",
            "software\\microsoft\\windows\\currentversion",
        ];
        const PREFIX: &[&str] = &[
            "software\\classes\\clsid",
            "software\\classes\\interface",
            "software\\classes\\typelib",
            "software\\microsoft\\windows\\currentversion\\uninstall",
            "software\\microsoft\\windows\\currentversion\\installer",
            "software\\microsoft\\windows\\currentversion\\sidebyside",
            "software\\microsoft\\windows nt\\currentversion",
        ];
        if EXACT.contains(&lower.as_str())
            || PREFIX
                .iter()
                .any(|critical| lower == *critical || lower.starts_with(&format!("{critical}\\")))
        {
            return Risk::Blocked;
        }
    }
    Risk::Safe
}

fn classify_file_str(input: &str) -> Risk {
    let expanded = expand_env(input);
    if has_parent_dir(&expanded) {
        return Risk::Blocked;
    }
    let path = normalize(&expanded);
    if path.is_empty() || !is_absolute(&path) || is_drive_root(&path) || path.starts_with(r"\\") {
        return Risk::Blocked;
    }
    if env_path("WINDIR").is_some_and(|root| has_root(&path, &format!("{root}\\temp"))) {
        return Risk::Safe;
    }
    if env_path("USERPROFILE").is_some_and(|root| path == root) {
        return Risk::Blocked;
    }
    if blocked_file_roots()
        .iter()
        .any(|root| has_root(&path, root))
    {
        return Risk::Blocked;
    }
    if safe_file_roots().iter().any(|root| has_root(&path, root)) {
        return Risk::Safe;
    }
    Risk::Outside
}

fn blocked_file_roots() -> Vec<String> {
    let mut roots: Vec<String> = [
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
    ]
    .iter()
    .filter_map(|name| env_path(name))
    .collect();
    if let Some(profile) = env_path("USERPROFILE") {
        for folder in [
            "Desktop",
            "Documents",
            "Downloads",
            "Pictures",
            "Music",
            "Videos",
            "Favorites",
            "Contacts",
            "Links",
            "Saved Games",
            "Searches",
            "OneDrive",
        ] {
            roots.push(format!("{profile}\\{}", folder.to_lowercase()));
        }
    }
    if let Some(appdata) = env_path("APPDATA") {
        roots.push(format!(
            "{appdata}\\microsoft\\windows\\start menu\\programs\\startup"
        ));
        roots.push(format!("{appdata}\\microsoft\\crypto"));
        roots.push(format!("{appdata}\\microsoft\\systemcertificates"));
    }
    if let Some(local) = env_path("LOCALAPPDATA") {
        roots.push(format!("{local}\\microsoft\\crypto"));
    }
    if let Some(programdata) = env_path("PROGRAMDATA") {
        roots.push(format!(
            "{programdata}\\microsoft\\windows\\start menu\\programs\\startup"
        ));
        roots.push(format!("{programdata}\\microsoft\\crypto"));
        roots.push(format!("{programdata}\\microsoft\\systemcertificates"));
    }
    roots
}

fn safe_file_roots() -> Vec<String> {
    let mut roots: Vec<String> = ["TEMP", "LOCALAPPDATA", "APPDATA", "PROGRAMDATA"]
        .iter()
        .filter_map(|name| env_path(name))
        .collect();
    if let Some(profile) = env_path("USERPROFILE") {
        roots.push(format!("{profile}\\appdata\\locallow"));
    }
    roots
}

fn env_path(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| normalize(&value.to_string_lossy()))
}

fn normalize(input: &str) -> String {
    let expanded = expand_env(input);
    let mut text = expanded.replace('/', "\\").to_lowercase();
    let unc = if let Some(rest) = text.strip_prefix(r"\\?\unc\") {
        text = format!(r"\\{rest}");
        true
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        text = rest.to_owned();
        false
    } else if let Some(rest) = text.strip_prefix(r"\\.\") {
        text = rest.to_owned();
        false
    } else {
        text.starts_with(r"\\")
    };
    let parts: Vec<&str> = text
        .split('\\')
        .map(|part| part.trim_end_matches([' ', '.']))
        .filter(|part| !part.is_empty())
        .collect();
    let joined = parts.join("\\");
    let mut text = if unc { format!(r"\\{joined}") } else { joined };
    while text.ends_with('\\') && !is_drive_root(&text) {
        text.pop();
    }
    text
}

fn is_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || (bytes.len() == 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\')
}

fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
}

fn has_root(path: &str, root: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    path == root || path.starts_with(&format!("{root}\\"))
}

fn has_parent_dir(input: &str) -> bool {
    input.split(['\\', '/']).any(|component| component == "..")
}

fn worst(a: Risk, b: Risk) -> Risk {
    match (a, b) {
        (Risk::Blocked, _) | (_, Risk::Blocked) => Risk::Blocked,
        (Risk::Outside, _) | (_, Risk::Outside) => Risk::Outside,
        _ => Risk::Safe,
    }
}

pub(crate) fn expand_env(input: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn env(name: &str) -> String {
        std::env::var(name).unwrap()
    }

    #[test]
    fn classifies_file_roots() {
        let temp = PathBuf::from(env("TEMP"));
        assert_eq!(classify_file(&temp.join("cache.tmp")), Risk::Safe);

        let local = PathBuf::from(env("LOCALAPPDATA"));
        assert_eq!(classify_file(&local.join("App\\cache.bin")), Risk::Safe);

        let windows = PathBuf::from(env("WINDIR"));
        assert_eq!(
            classify_file(&windows.join("System32\\kernel32.dll")),
            Risk::Blocked
        );
        assert_eq!(
            classify_file(&windows.join("Temp\\install.tmp")),
            Risk::Safe
        );
        assert_eq!(
            classify_file(Path::new(r"C:\ChipCleanOutsideRoot\data.bin")),
            Risk::Outside
        );
        assert_eq!(classify_file(Path::new(r"C:\")), Risk::Blocked);
        assert_eq!(
            classify_file(Path::new(r"C:\Windows\..\Users\Public\x")),
            Risk::Blocked
        );
        assert_eq!(
            classify_file(Path::new(r"\\server\share\data.bin")),
            Risk::Blocked
        );
        let verbatim = format!(r"\\?\{}\System32\kernel32.dll", windows.display());
        assert_eq!(classify_file(Path::new(&verbatim)), Risk::Blocked);
        let padded = format!(r"{}\System32. \kernel32.dll", windows.display());
        assert_eq!(classify_file(Path::new(&padded)), Risk::Blocked);
        let startup = PathBuf::from(env("APPDATA"))
            .join(r"Microsoft\Windows\Start Menu\Programs\Startup\evil.lnk");
        assert_eq!(classify_file(&startup), Risk::Blocked);
        assert_eq!(
            classify_file(Path::new(r"\Windows\System32\kernel32.dll")),
            Risk::Blocked
        );
        assert_eq!(
            classify_file(Path::new(r"Windows\System32\kernel32.dll")),
            Risk::Blocked
        );
    }

    #[test]
    fn classifies_file_patterns() {
        let temp = PathBuf::from(env("TEMP"));
        let pattern = format!("{}\\*.*", temp.display());
        assert_eq!(classify_file_pattern(&pattern), Risk::Safe);

        let windows = PathBuf::from(env("WINDIR"));
        let blocked = format!("{}\\System32\\*", windows.display());
        assert_eq!(classify_file_pattern(&blocked), Risk::Blocked);

        let outside = r"C:\ChipCleanOutsideRoot\*\cache";
        assert_eq!(classify_file_pattern(outside), Risk::Outside);
    }

    #[test]
    fn classifies_registry_roots() {
        assert_eq!(
            classify_registry(EditorHive::Hkcu, r"Software\Vendor\App", false),
            Risk::Safe
        );
        assert_eq!(
            classify_registry(EditorHive::Hklm, r"SOFTWARE\Vendor\App", false),
            Risk::Safe
        );
        assert_eq!(
            classify_registry(EditorHive::Hkcu, r"Environment", false),
            Risk::Blocked
        );
        assert_eq!(
            classify_registry(EditorHive::Hklm, r"SYSTEM\CurrentControlSet", false),
            Risk::Blocked
        );
        assert_eq!(
            classify_registry(EditorHive::Hkcu, r"Console", false),
            Risk::Outside
        );
        assert_eq!(
            classify_registry(EditorHive::Hkcu, r"Software", true),
            Risk::Blocked
        );
        assert_eq!(
            classify_registry(
                EditorHive::Hklm,
                r"Software\Microsoft\Windows NT\CurrentVersion\ProfileList",
                false
            ),
            Risk::Blocked
        );
    }

    #[test]
    fn blocks_removeself_on_critical_subtrees() {
        for path in [
            r"SOFTWARE\Classes",
            r"SOFTWARE\Classes\CLSID",
            r"SOFTWARE\Classes\CLSID\{GUID}",
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Vendor",
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        ] {
            assert_eq!(
                classify_registry(EditorHive::Hklm, path, true),
                Risk::Blocked,
                "должно блокироваться: {path}"
            );
        }
        assert_eq!(
            classify_registry(EditorHive::Hkcu, r"Software\Classes", true),
            Risk::Blocked
        );
        assert_eq!(
            classify_registry(EditorHive::Hkcu, r"Software\Vendor\App", true),
            Risk::Safe
        );
    }

    #[test]
    fn classify_file_canonicalizes_safe_paths() {
        let temp = tempfile::tempdir().unwrap();
        let link = temp.path().join("junction");
        let windows = env("WINDIR");
        let target = format!("{windows}\\System32");
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        if !output.status.success() {
            // Junction creation can be blocked by policy; nothing to assert then.
            return;
        }
        // Raw path looks Safe (under TEMP), but it resolves into System32.
        assert_eq!(classify_file(&link.join("kernel32.dll")), Risk::Blocked);
        let _ = std::fs::remove_dir(&link);
    }
}
