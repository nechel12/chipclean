//! Обновление базы правил сообщества (winapp2.ini) из интернета.
//!
//! База скачивается с официального открытого репозитория сообщества
//! (MoscaDotTo/Winapp2) и сохраняется локально атомарной заменой.
//! Скачанный файл — это только данные правил; он никогда не выполняется.
//! Никакие локальные данные наружу не отправляются.

use std::io::Read;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde_json::json;

use crate::config;
use crate::core::{fs_atomic, journal::Journal};

/// Официальный источник актуальной базы правил.
pub const WINAPP2_URL: &str =
    "https://raw.githubusercontent.com/MoscaDotTo/Winapp2/master/Winapp2.ini";

/// Верхняя граница размера базы (защита от аномального ответа).
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

/// Результат успешного обновления.
#[derive(Debug)]
pub struct UpdateOutcome {
    /// Размер сохранённой базы в байтах.
    pub bytes: u64,
}

/// Скачивает свежую базу winapp2.ini и атомарно заменяет локальный файл.
pub fn download_winapp2() -> Result<UpdateOutcome> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build();
    let agent = config.new_agent();
    let response = agent
        .get(WINAPP2_URL)
        .call()
        .context("не удалось подключиться к серверу базы")?;

    let status = response.status();
    if status != 200 {
        bail!("сервер ответил кодом {status}");
    }

    let mut body = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .context("не удалось прочитать ответ сервера")?;

    if body.is_empty() {
        bail!("пустой ответ сервера");
    }
    if body.len() > MAX_BYTES {
        bail!("база слишком большая (больше 20 МБ)");
    }
    validate_ini(&body)?;

    let path = config::community_db_path();
    fs_atomic::write_bytes_atomic(&path, &body)
        .with_context(|| format!("не удалось сохранить базу в {}", path.display()))?;

    let mut journal = Journal::open()?;
    journal.record(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "community_db_updated",
        "source": WINAPP2_URL,
        "bytes": body.len(),
        "path": path.display().to_string(),
    }))?;
    journal.flush()?;

    Ok(UpdateOutcome {
        bytes: body.len() as u64,
    })
}

/// Минимальная проверка, что ответ действительно является базой правил,
/// а не HTML-страницей ошибки или произвольным текстом.
fn validate_ini(body: &[u8]) -> Result<()> {
    let text = String::from_utf8_lossy(body);
    if text.contains("<html") || text.starts_with("404") || text.contains("Not Found") {
        bail!("ответ сервера не похож на базу правил");
    }
    if !text.contains('[') {
        bail!("в ответе нет секций базы правил");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_html_and_404_responses() {
        assert!(validate_ini(b"<html><body>Not Found</body></html>").is_err());
        assert!(validate_ini(b"404: Not Found").is_err());
        assert!(validate_ini(b"hello world").is_err());
    }

    #[test]
    fn accepts_section_like_ini() {
        assert!(validate_ini(b"[Temporary Files]\nFileKey1=%TEMP%\\|*").is_ok());
    }
}
