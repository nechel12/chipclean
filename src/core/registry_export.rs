//! Экспорт ключей реестра в текст `.reg` без внешней утилиты `reg.exe`.

use anyhow::Result;
use windows_registry::{Key, Type, Value};

/// Экспортирует ключ (с поддеревом) в формат .reg. Пустой путь — корень куста.
pub fn export_key_text(root: &Key, hive_name: &str, key_path: &str) -> Result<String> {
    let mut out = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    if key_path.trim().is_empty() {
        out.push_str(&format!("[{}]\r\n", hive_name));
        if let Ok(subkeys) = root.keys() {
            for sub in subkeys {
                export_tree(root, hive_name, &sub, &mut out)?;
            }
        }
    } else {
        export_tree(root, hive_name, key_path, &mut out)?;
    }
    Ok(out)
}

fn export_tree(root: &Key, hive_name: &str, path: &str, out: &mut String) -> Result<()> {
    let Ok(key) = root.open(path) else {
        return Ok(());
    };

    out.push_str(&format!("[{hive_name}\\{path}]\r\n"));
    if let Ok(values) = key.values() {
        for (name, value) in values {
            append_value(out, &name, &value);
        }
    }
    if let Ok(subkeys) = key.keys() {
        for sub in subkeys {
            export_tree(root, hive_name, &format!("{path}\\{sub}"), out)?;
        }
    }
    Ok(())
}

fn append_value(out: &mut String, name: &str, value: &Value) {
    let key = if name.is_empty() {
        "@".to_owned()
    } else {
        format!("\"{}\"", escape(name))
    };
    let bytes: &[u8] = value;

    match value.ty() {
        Type::U32 => {
            let mut raw = [0u8; 4];
            let len = bytes.len().min(4);
            raw[..len].copy_from_slice(&bytes[..len]);
            let number = u32::from_le_bytes(raw);
            out.push_str(&format!("{key}=dword:{number:08x}\r\n"));
        }
        Type::U64 => {
            let len = bytes.len().min(8);
            out.push_str(&format!("{key}=hex(b):{}\r\n", hex_bytes(&bytes[..len])));
        }
        Type::String => {
            let text = wide_to_string(value);
            out.push_str(&format!("{key}=\"{}\"\r\n", escape_value(&text)));
        }
        Type::ExpandString => {
            out.push_str(&format!("{key}=hex(2):{}\r\n", hex_bytes(bytes)));
        }
        Type::MultiString => {
            out.push_str(&format!("{key}=hex(7):{}\r\n", hex_bytes(bytes)));
        }
        _ => {
            out.push_str(&format!("{key}=hex:{}\r\n", hex_bytes(bytes)));
        }
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn wide_to_string(value: &Value) -> String {
    let wide = value.as_wide();
    let end = wide.iter().position(|&unit| unit == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_value(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_registry::CURRENT_USER;

    #[test]
    fn export_includes_header_and_default_value() {
        let path = r"Software\ChipClean\ExportTest\Sub";
        let _ = CURRENT_USER.remove_tree(r"Software\ChipClean\ExportTest");
        let key = CURRENT_USER.create(path).unwrap();
        key.set_string("", "default-value").unwrap();
        key.set_u32("Dword", 42).unwrap();

        let text = export_key_text(CURRENT_USER, "HKEY_CURRENT_USER", path).unwrap();
        assert!(text.starts_with("Windows Registry Editor Version 5.00"));
        assert!(text.contains("[HKEY_CURRENT_USER\\"));
        assert!(
            text.contains("@=\"default-value\""),
            "в тексте нет default-значения:\n{text}"
        );
        assert!(text.contains("\"Dword\"=dword:0000002a"));

        let _ = CURRENT_USER.remove_tree(r"Software\ChipClean\ExportTest");
    }
}
