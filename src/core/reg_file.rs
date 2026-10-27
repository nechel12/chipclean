use crate::core::registry_editor::EditorHive;

pub(crate) fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let mut body = &bytes[2..];
        body = &body[..body.len() - body.len() % 2];
        let units: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        let mut body = &bytes[2..];
        body = &body[..body.len() - body.len() % 2];
        let units: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

pub(crate) fn parse_key_path(key: &str) -> Option<(EditorHive, String)> {
    let key = key.trim().trim_matches('\\');
    let (hive, path) = key.split_once('\\').unwrap_or((key, ""));
    let hive = match hive.to_ascii_uppercase().as_str() {
        "HKEY_CURRENT_USER" | "HKCU" => EditorHive::Hkcu,
        "HKEY_LOCAL_MACHINE" | "HKLM" => EditorHive::Hklm,
        "HKEY_CLASSES_ROOT" | "HKCR" => EditorHive::Hkcr,
        "HKEY_USERS" | "HKU" => EditorHive::Hku,
        "HKEY_CURRENT_CONFIG" | "HKCC" => EditorHive::Hkcc,
        _ => return None,
    };
    if path.is_empty() {
        return None;
    }
    Some((hive, path.to_owned()))
}

pub(crate) fn unescape_name(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => output.push('\\'),
            Some('"') => output.push('"'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_hives() {
        assert_eq!(
            parse_key_path(r"HKEY_CURRENT_USER\Software\A"),
            Some((EditorHive::Hkcu, r"Software\A".to_owned()))
        );
        assert_eq!(
            parse_key_path(r"HKLM\SOFTWARE\B"),
            Some((EditorHive::Hklm, r"SOFTWARE\B".to_owned()))
        );
        assert_eq!(parse_key_path("HKEY_FAKE\\X"), None);
    }

    #[test]
    fn unescapes_value_names() {
        assert_eq!(unescape_name(r#"a\\b\"c"#), r#"a\b"c"#);
    }
}
