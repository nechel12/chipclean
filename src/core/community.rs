use std::{fs, path::Path};

use anyhow::{Context, Result, bail};

use crate::core::{
    privacy::{DataTarget, FileSpec, RegSpec, expand_pattern},
    registry_editor::EditorHive,
};

struct Section {
    name: String,
    values: Vec<(String, String)>,
}

impl Section {
    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn starts_with(&self, prefix: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|(key, _)| key.to_ascii_lowercase().starts_with(prefix))
            .map(|(_, value)| value.as_str())
            .collect()
    }

    fn default_enabled(&self) -> bool {
        self.get("Default")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    }
}

pub fn parse_database(path: &Path) -> Result<Vec<DataTarget>> {
    let bytes =
        fs::read(path).with_context(|| format!("не удалось прочитать {}", path.display()))?;
    if bytes.is_empty() {
        bail!("Файл базы пуст");
    }
    let text = decode(&bytes);
    let sections = parse_sections(&text);
    if sections.is_empty() {
        bail!("В файле не найдено ни одной секции");
    }

    let mut targets = Vec::new();
    for (index, section) in sections.into_iter().enumerate() {
        let files = parse_files(&section);
        let registry = parse_registry(&section);
        if files.is_empty() && registry.is_empty() {
            continue;
        }
        if !detected(&section) {
            continue;
        }
        let group = group_label(section.get("LangSecRef").unwrap_or_default());
        let excludes = parse_excludes(&section);
        targets.push(DataTarget {
            id: format!("winapp2-{index}"),
            title: section.name.clone(),
            summary: format!("{} файлов · {} ключей", files.len(), registry.len()),
            group,
            warning: section.get("Warning").map(str::to_owned),
            default_enabled: section.default_enabled(),
            files,
            history: Vec::new(),
            registry,
            exclude_files: excludes.0,
            exclude_registry: excludes.1,
        });
    }

    targets.sort_by(|left, right| {
        left.group
            .cmp(&right.group)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });
    Ok(targets)
}

fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let words: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes(*chunk))
            .collect();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes))
            .into_owned()
    }
}

fn parse_sections(text: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current: Option<Section> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            let name = line[1..line.len() - 1].trim();
            current = Some(Section {
                name: name.to_owned(),
                values: Vec::new(),
            });
            continue;
        }
        if let Some(section) = current.as_mut()
            && let Some((key, value)) = line.split_once('=')
        {
            section
                .values
                .push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    if let Some(section) = current {
        sections.push(section);
    }
    sections
}

fn detected(section: &Section) -> bool {
    // DetectOS is an OS-version constraint, not a path; it is not evaluated
    // here, but it must not be treated as a file path (which never matches and
    // would hide sections that only declare DetectOS).
    let detects: Vec<&str> = section
        .values
        .iter()
        .filter(|(key, _)| {
            let key = key.to_ascii_lowercase();
            key.starts_with("detect") && !key.starts_with("detectos")
        })
        .map(|(_, value)| value.as_str())
        .collect();
    if detects.is_empty() {
        return true;
    }
    detects.iter().any(|detect| {
        let value = detect.split('|').next().unwrap_or_default().trim();
        if let Some((hive, path)) = parse_hive_path(value) {
            return crate::core::registry_editor::open_key(hive, &path).is_ok();
        }
        !expand_pattern(value).is_empty()
    })
}

fn parse_files(section: &Section) -> Vec<FileSpec> {
    let mut files = Vec::new();
    for value in section.starts_with("filekey") {
        if let Some(file) = parse_file_key(value) {
            files.push(file);
        }
    }
    files
}

fn parse_file_key(value: &str) -> Option<FileSpec> {
    let mut parts = value.split('|');
    let mut path = parts.next()?.trim().to_owned();
    if path.is_empty() {
        return None;
    }
    let mut recurse = false;
    let mut regex = false;
    for part in parts {
        let upper = part.trim().to_ascii_uppercase();
        match upper.as_str() {
            "RECURSE" => recurse = true,
            "REGEX" => regex = true,
            "REMOVESELF" => {}
            _ => {
                let mask = part.trim();
                if !mask.is_empty() {
                    let base = path
                        .trim_end_matches("\\*.*")
                        .trim_end_matches("\\*")
                        .trim_end_matches('\\');
                    path = format!("{base}\\{mask}");
                }
            }
        }
    }
    if regex {
        return None;
    }
    Some(FileSpec {
        pattern: path,
        recurse,
    })
}

fn parse_registry(section: &Section) -> Vec<RegSpec> {
    let mut registry = Vec::new();
    for value in section.starts_with("regkey") {
        if let Some(reg) = parse_reg_key(value) {
            registry.push(reg);
        }
    }
    registry
}

fn parse_reg_key(value: &str) -> Option<RegSpec> {
    let mut parts = value.split('|');
    let full = parts.next()?.trim();
    let (hive, path) = parse_hive_path(full)?;
    // Cleaning is limited to HKCU/HKLM: HKCR is a merged view, HKU holds other
    // users' hives and HKCC is volatile. Such RegKey entries are documented as
    // unsupported instead of being silently half-applied.
    if !matches!(hive, EditorHive::Hkcu | EditorHive::Hklm) {
        return None;
    }
    let mut reg_value = None;
    let mut recurse = false;
    let mut remove_self = false;
    let mut all_values = false;
    for part in parts {
        let part = part.trim();
        let upper = part.to_ascii_uppercase();
        match upper.as_str() {
            "RECURSE" => recurse = true,
            "REMOVESELF" => remove_self = true,
            "ALL" | "*" => all_values = true,
            "" => {}
            _ => {
                if reg_value.is_none() && !all_values {
                    reg_value = Some(part.to_owned());
                }
            }
        }
    }
    if remove_self || all_values {
        reg_value = None;
    }
    Some(RegSpec {
        hive,
        path,
        value: reg_value,
        recurse,
        remove_self,
    })
}

fn parse_excludes(section: &Section) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut registry = Vec::new();
    for value in section.starts_with("excludekey") {
        let value = value.trim();
        let first = value.split('|').next().unwrap_or_default().trim();
        if let Some(rest) = value.strip_prefix("FILE|") {
            let mut parts = rest
                .split('|')
                .map(str::trim)
                .filter(|part| !part.is_empty() && !part.eq_ignore_ascii_case("REGEX"));
            if let Some(dir) = parts.next() {
                let combined = match parts.next() {
                    Some(mask) => format!("{dir}\\{mask}"),
                    None => dir.to_owned(),
                };
                files.push(combined);
            }
        } else if let Some(rest) = value.strip_prefix("REG|") {
            registry.push(rest.split('|').next().unwrap_or_default().trim().to_owned());
        } else if parse_hive_path(first).is_some() {
            registry.push(first.to_owned());
        } else if !first.is_empty() {
            files.push(first.to_owned());
        }
    }
    (files, registry)
}

fn parse_hive_path(value: &str) -> Option<(EditorHive, String)> {
    let (hive, path) = value.split_once('\\')?;
    let hive = match hive.to_ascii_uppercase().as_str() {
        "HKCU" => EditorHive::Hkcu,
        "HKLM" => EditorHive::Hklm,
        "HKCR" => EditorHive::Hkcr,
        "HKU" => EditorHive::Hku,
        "HKCC" => EditorHive::Hkcc,
        _ => return None,
    };
    Some((hive, path.trim_matches('\\').to_owned()))
}

fn group_label(lang: &str) -> String {
    match lang.trim() {
        "3021" => "Приложения",
        "3022" => "Интернет",
        "3023" => "Windows",
        "3024" => "Игры",
        "3025" => "Мультимедиа",
        "3026" => "Утилиты",
        _ => "База сообщества",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_small_database() {
        let text = r#"
[Test App]
LangSecRef=3021
Default=True
Detect=HKCU\Software\ChipCleanTestDetect
FileKey1=%TEMP%\ChipCleanCommunityTest|*.*|RECURSE
RegKey1=HKCU\Software\ChipCleanTest|Value
"#;
        let sections = parse_sections(text);
        assert_eq!(sections.len(), 1);
        let files = parse_files(&sections[0]);
        let registry = parse_registry(&sections[0]);
        assert_eq!(files.len(), 1);
        assert!(files[0].recurse);
        assert_eq!(registry.len(), 1);
        assert_eq!(registry[0].value.as_deref(), Some("Value"));
        assert!(!detected(&sections[0]));
    }

    #[test]
    fn skips_unsupported_hives_but_keeps_their_excludes() {
        let text = r#"
[Extra Hives]
RegKey1=HKCR\ChipCleanTest\Classes|Value
RegKey2=HKU\ChipCleanTest\Software|ALL
RegKey3=HKCC\ChipCleanTest\Software|REMOVESELF
ExcludeKey1=HKCR\ChipCleanTest\Classes\Keep
ExcludeKey2=HKU\ChipCleanTest\Software\Keep
ExcludeKey3=%TEMP%\file.txt
"#;
        let sections = parse_sections(text);
        assert!(parse_registry(&sections[0]).is_empty());

        let (files, registry) = parse_excludes(&sections[0]);
        assert_eq!(files, vec![r"%TEMP%\file.txt".to_owned()]);
        assert_eq!(
            registry,
            vec![
                r"HKCR\ChipCleanTest\Classes\Keep".to_owned(),
                r"HKU\ChipCleanTest\Software\Keep".to_owned(),
            ]
        );
    }

    #[test]
    fn parses_registry_flags() {
        let all = parse_reg_key(r"HKCU\Software\ChipCleanTest|*|RECURSE").unwrap();
        assert!(all.value.is_none());
        assert!(all.recurse);
        assert!(!all.remove_self);

        let tree = parse_reg_key(r"HKLM\Software\ChipCleanTest|REMOVESELF").unwrap();
        assert!(tree.remove_self);
        assert!(tree.value.is_none());

        let value = parse_reg_key(r"HKCU\Software\ChipCleanTest|Value|RECURSE").unwrap();
        assert_eq!(value.value.as_deref(), Some("Value"));
        assert!(value.recurse);
        assert!(!value.remove_self);

        let bare = parse_reg_key(r"HKCU\Software\ChipCleanTest").unwrap();
        assert!(bare.value.is_none());
        assert!(!bare.remove_self);
    }

    #[test]
    fn file_key_combines_mask_even_with_wildcard_path() {
        let text = "[Masked]\nFileKey1=%TEMP%\\App\\*|*.log|RECURSE\n";
        let sections = parse_sections(text);
        let files = parse_files(&sections[0]);
        assert_eq!(files.len(), 1);
        assert!(files[0].pattern.ends_with(r"\*.log"));
        assert!(!files[0].pattern.contains(r"\*\"));
        assert!(files[0].recurse);
    }

    #[test]
    fn exclude_key_keeps_the_file_mask() {
        let text = "[Excluded]\nExcludeKey1=FILE|%TEMP%\\App|keep.log\nExcludeKey2=FILE|%TEMP%\\App\\sub\n";
        let sections = parse_sections(text);
        let (files, _) = parse_excludes(&sections[0]);
        assert_eq!(
            files,
            vec![
                r"%TEMP%\App\keep.log".to_owned(),
                r"%TEMP%\App\sub".to_owned(),
            ]
        );
    }

    #[test]
    fn detectos_does_not_hide_sections() {
        let text = "[OS Only]\nDetectOS=6.1\nFileKey1=%TEMP%\\x|*.*\n";
        let sections = parse_sections(text);
        assert!(detected(&sections[0]));
    }
}
