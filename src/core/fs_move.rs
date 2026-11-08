use std::{ffi::OsString, fs, path::Path};

use anyhow::{Context, Result, anyhow, bail};

pub fn move_path(source: &Path, destination: &Path) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        bail!("цель уже существует: {}", destination.display());
    }
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_err) => match move_path_copy(source, destination) {
            Ok(()) => Ok(()),
            Err(copy_err) => Err(copy_err).with_context(|| {
                format!(
                    "не удалось переместить {} в {} (переименование: {rename_err})",
                    source.display(),
                    destination.display()
                )
            }),
        },
    }
}

fn move_path_copy(source: &Path, destination: &Path) -> Result<()> {
    move_path_copy_with(source, destination, verify_tree)
}

fn move_path_copy_with(
    source: &Path,
    destination: &Path,
    verify: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        bail!("цель уже существует: {}", destination.display());
    }

    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("не удалось прочитать {}", source.display()))?;
    if is_reparse(&metadata) {
        bail!(
            "символические ссылки и reparse points запрещены: {}",
            source.display()
        );
    }

    let copied = copy_tree(source, destination).and_then(|()| verify(source, destination));
    if let Err(err) = copied {
        let _ = remove_path(destination);
        return Err(err);
    }

    if let Err(err) = remove_path(source) {
        return match remove_path(destination) {
            Ok(()) => Err(err).with_context(|| {
                format!(
                    "не удалось удалить исходный объект; копия удалена: {}",
                    source.display()
                )
            }),
            Err(cleanup_err) => Err(anyhow!(
                "не удалось удалить {}: {err}; копия {} осталась: {cleanup_err}",
                source.display(),
                destination.display()
            )),
        };
    }

    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("не удалось прочитать {}", source.display()))?;
    if is_reparse(&metadata) {
        bail!(
            "символические ссылки и reparse points запрещены: {}",
            source.display()
        );
    }

    if metadata.is_dir() {
        fs::create_dir(destination)
            .with_context(|| format!("не удалось создать {}", destination.display()))?;
        let entries = fs::read_dir(source)
            .with_context(|| format!("не удалось прочитать {}", source.display()))?;
        for entry in entries {
            let entry =
                entry.with_context(|| format!("не удалось прочитать {}", source.display()))?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        let written = fs::copy(source, destination).with_context(|| {
            format!(
                "не удалось скопировать {} в {}",
                source.display(),
                destination.display()
            )
        })?;
        if written != metadata.len() {
            bail!(
                "скопировано {written} из {} байт: {}",
                metadata.len(),
                source.display()
            );
        }
    } else {
        bail!("неподдерживаемый тип объекта: {}", source.display());
    }

    Ok(())
}

fn verify_tree(source: &Path, destination: &Path) -> Result<()> {
    let source_meta = fs::symlink_metadata(source)
        .with_context(|| format!("не удалось проверить {}", source.display()))?;
    let destination_meta = fs::symlink_metadata(destination)
        .with_context(|| format!("не удалось проверить {}", destination.display()))?;

    if source_meta.is_dir() != destination_meta.is_dir() {
        bail!("тип объекта не совпал: {}", destination.display());
    }

    if source_meta.is_dir() {
        let mut source_names = entry_names(source)?;
        let mut destination_names = entry_names(destination)?;
        source_names.sort();
        destination_names.sort();
        if source_names != destination_names {
            bail!("состав каталога не совпал: {}", destination.display());
        }
        for name in source_names {
            verify_tree(&source.join(&name), &destination.join(&name))?;
        }
    } else if source_meta.len() != destination_meta.len() {
        bail!(
            "размер не совпал: {} ({} != {})",
            destination.display(),
            destination_meta.len(),
            source_meta.len()
        );
    }

    Ok(())
}

fn entry_names(path: &Path) -> Result<Vec<OsString>> {
    let mut names = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("не удалось прочитать {}", path.display()))?
    {
        names.push(
            entry
                .with_context(|| format!("не удалось прочитать {}", path.display()))?
                .file_name(),
        );
    }
    Ok(names)
}

fn remove_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("не удалось прочитать {}", path.display()))?;
    if metadata.is_dir() && !is_reparse(&metadata) {
        fs::remove_dir_all(path)
            .with_context(|| format!("не удалось удалить папку {}", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("не удалось удалить {}", path.display()))?;
    }
    Ok(())
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
    fn move_path_moves_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        let destination = dir.path().join("moved.bin");
        fs::write(&source, b"payload").unwrap();

        move_path(&source, &destination).unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"payload");
    }

    #[test]
    fn copy_fallback_moves_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let destination = dir.path().join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("a.bin"), b"1234").unwrap();
        fs::write(source.join("nested").join("b.bin"), b"567890").unwrap();

        move_path_copy(&source, &destination).unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read(destination.join("a.bin")).unwrap(), b"1234");
        assert_eq!(
            fs::read(destination.join("nested").join("b.bin")).unwrap(),
            b"567890"
        );
    }

    #[test]
    fn copy_fallback_refuses_an_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        let destination = dir.path().join("destination.bin");
        fs::write(&source, b"source").unwrap();
        fs::write(&destination, b"destination").unwrap();

        assert!(move_path_copy(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert_eq!(fs::read(&destination).unwrap(), b"destination");
    }

    #[test]
    fn copy_fallback_removes_the_copy_when_verification_fails() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        let destination = dir.path().join("destination.bin");
        fs::write(&source, b"payload").unwrap();

        let result =
            move_path_copy_with(&source, &destination, |_, _| bail!("verification failed"));

        assert!(result.is_err());
        assert_eq!(fs::read(&source).unwrap(), b"payload");
        assert!(!destination.exists());
    }
}
