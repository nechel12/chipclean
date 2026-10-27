use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

pub fn canonical_existing(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).with_context(|| format!("не удалось проверить {}", path.display()))
}

pub fn canonical_destination(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("Путь должен быть абсолютным: {}", path.display());
    }

    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Некорректный путь: {}", path.display()))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Некорректный путь: {}", path.display()))?;
    if !parent.is_dir() {
        bail!("Каталог не найден: {}", parent.display());
    }

    let parent = fs::canonicalize(parent)
        .with_context(|| format!("не удалось проверить {}", parent.display()))?;
    Ok(parent.join(name))
}

pub fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

pub fn is_within(path: &Path, root: &Path) -> bool {
    let path = normalize(path);
    let root = normalize(root);
    path == root || path.starts_with(&format!("{root}\\"))
}

pub fn ensure_within(path: &Path, roots: &[PathBuf]) -> Result<()> {
    let mut checked = false;

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        checked = true;
        let root = canonical_existing(root)?;
        if is_within(path, &root) {
            return Ok(());
        }
    }

    if !checked {
        bail!("Разрешённые каталоги не найдены");
    }
    bail!("Путь вне разрешённых каталогов: {}", path.display())
}

pub fn ensure_direct_child(path: &Path, roots: &[PathBuf]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Некорректный путь: {}", path.display()))?;
    let parent = canonical_existing(parent)?;
    let mut checked = false;

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        checked = true;
        let root = canonical_existing(root)?;
        if same_path(&parent, &root) {
            return Ok(());
        }
    }

    if !checked {
        bail!("Разрешённые каталоги не найдены");
    }
    bail!("Путь вне разрешённых каталогов: {}", path.display())
}

pub fn ensure_existing_within(path: &Path, roots: &[PathBuf]) -> Result<()> {
    let canonical = canonical_existing(path)?;
    ensure_within(&canonical, roots)
}

pub fn ensure_not_reparse(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("не удалось прочитать {}", path.display()))?;
    if is_reparse(&metadata) {
        bail!(
            "Символические ссылки и reparse points запрещены: {}",
            path.display()
        );
    }
    Ok(())
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment_is_case_insensitive() {
        assert!(is_within(
            Path::new(r"C:\Users\Test\AppData\Local\Temp\file"),
            Path::new(r"c:\users\test\appdata\local\temp")
        ));
        assert!(!is_within(
            Path::new(r"C:\Users\Test\AppData\Local\Temp2\file"),
            Path::new(r"C:\Users\Test\AppData\Local\Temp")
        ));
    }

    #[test]
    fn destination_requires_existing_parent() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("Startup");
        fs::create_dir_all(&parent).unwrap();

        let destination = canonical_destination(&parent.join("item.lnk")).unwrap();
        assert!(destination.ends_with("item.lnk"));

        let missing = canonical_destination(&dir.path().join("missing").join("item.lnk"));
        assert!(missing.is_err());
    }

    #[test]
    fn direct_child_requires_known_parent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Startup");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let file = root.join("item.lnk");
        fs::write(&file, b"test").unwrap();

        assert!(ensure_direct_child(&file, std::slice::from_ref(&root)).is_ok());
        assert!(ensure_direct_child(&nested.join("item.lnk"), &[root]).is_err());
    }
}
