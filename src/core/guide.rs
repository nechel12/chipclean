//! Современная справка в формате HTML5 (вместо устаревшего CHM).
//!
//! Генерирует автономный HTML-файл на текущем языке и открывает его
//! в браузере по умолчанию.

use anyhow::{Context, Result};

use crate::config;
use crate::i18n::{Lang, current};

/// Генерирует HTML-руководство и открывает его в браузере.
pub fn open_guide() -> Result<std::path::PathBuf> {
    let lang = current();
    let html = guide_html(lang);

    let dir = config::log_dir().join("guide");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("не удалось создать {}", dir.display()))?;

    let path = dir.join(format!("chipclean-guide-{}.html", lang.code()));
    std::fs::write(&path, html)
        .with_context(|| format!("не удалось записать {}", path.display()))?;

    std::process::Command::new("cmd")
        .args(["/C", "start", "", &path.display().to_string()])
        .spawn()
        .context("не удалось открыть браузер")?;

    Ok(path)
}

fn guide_html(lang: Lang) -> String {
    let (title, intro, sections, safety, license) = match lang {
        Lang::Ru => (
            "ChipClean — Руководство",
            "ChipClean — сканер и безопасная очистка Windows, вдохновлённая Reg Organizer. Всё работает локально.",
            vec![
                ("Очистка системы", "Временные файлы, кэши, логи, отчёты об ошибках, обновления Windows, корзина, загрузки."),
                ("Реестр", "Безопасная проверка, редактор с поиском и заменой, импорт .reg с предпросмотром, снимки и откат."),
                ("Автозагрузка", "Run/RunOnce, папки автозагрузки, задачи планировщика, службы — отключение с карантином."),
                ("Личные данные", "История браузеров, недавние документы и списки файлов."),
                ("Чистка сообществом", "Правила winapp2.ini с обновлением базы из интернета."),
                ("Твики", "Скрытые настройки Windows по образцу Reg Organizer; перед изменением — резервная копия .reg."),
                ("Фоновая очистка", "Ежедневная задача и проверка при входе через Планировщик заданий."),
                ("Экспресс-проверка", "Один запуск всех сканов."),
                ("Анализ диска", "Крупнейшие файлы и занятость тома."),
            ],
            vec![
                "Все изменения реестра и файлов пишутся в журнал операций.",
                "Перед изменением значений твиков создаётся резервная копия .reg.",
                "Удаление только после предпросмотра и подтверждения.",
                "Программа не отправляет данные наружу; база правил скачивается только по явной команде.",
            ],
            vec![
                "ChipClean — свободная программа (Apache-2.0).",
                "Твики и совместимые правила перенесены из открытого конфигурационного файла Reg Organizer (ChemTable Software).",
                "Используйте на свой риск; проверяйте выбранные секции очистки.",
            ],
        ),
        Lang::En => (
            "ChipClean — User Guide",
            "ChipClean — a scanner and safe Windows cleaner inspired by Reg Organizer. Everything runs locally.",
            vec![
                ("System Cleanup", "Temporary files, caches, logs, error reports, Windows updates, Recycle Bin, downloads."),
                ("Registry", "Safe check, editor with search and replace, .reg import with preview, snapshots and rollback."),
                ("Startup", "Run/RunOnce, startup folders, Task Scheduler tasks, services — disable with quarantine."),
                ("Privacy", "Browser history, recent documents and file lists."),
                ("Community Cleanup", "winapp2.ini rules with online database updates."),
                ("Tweaks", "Hidden Windows settings following Reg Organizer; .reg backup before each change."),
                ("Background Cleanup", "Daily task and logon check via Task Scheduler."),
                ("Express Check", "Run all scans with one button."),
                ("Disk Analysis", "Largest files and volume usage."),
            ],
            vec![
                "All registry and file changes are written to the operations journal.",
                "A .reg backup is created before applying tweak values.",
                "Deletion only after preview and confirmation.",
                "The program does not send data anywhere; the rules database is downloaded only on explicit command.",
            ],
            vec![
                "ChipClean is free software (Apache-2.0).",
                "Tweaks and compatible rules were ported from the open configuration file of Reg Organizer (ChemTable Software).",
                "Use at your own risk; review selected cleanup sections.",
            ],
        ),
    };

    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"");
    html.push_str(lang.code());
    html.push_str("\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    html.push_str("<title>");
    html.push_str(title);
    html.push_str("</title><style>");
    html.push_str(
        "body{font-family:Segoe UI,system-ui,sans-serif;max-width:860px;margin:40px auto;padding:0 20px;color:#222;line-height:1.55}
h1{font-size:1.8rem;border-bottom:2px solid #e5484d;padding-bottom:10px}
h2{font-size:1.2rem;margin-top:28px;color:#333}
.card{background:#fafafa;border:1px solid #e5e5e5;border-radius:10px;padding:12px 16px;margin:8px 0}
.note{background:#fff7f7;border-left:4px solid #e5484d;padding:10px 14px;border-radius:0 8px 8px 0;margin-top:24px}
ul{margin:6px 0 0 20px}
code{background:#f0f0f0;padding:1px 5px;border-radius:4px}",
    );
    html.push_str("</style></head><body>");
    html.push_str(&format!("<h1>{title}</h1>"));
    html.push_str(&format!("<p>{intro}</p>"));

    html.push_str("<h2>");
    html.push_str(if lang == Lang::Ru { "Возможности" } else { "Features" });
    html.push_str("</h2>");
    for (name, desc) in sections {
        html.push_str(&format!(
            "<div class=\"card\"><strong>{name}</strong><div>{desc}</div></div>"
        ));
    }

    html.push_str("<h2>");
    html.push_str(if lang == Lang::Ru {
        "Гарантии безопасности"
    } else {
        "Safety guarantees"
    });
    html.push_str("</h2><ul>");
    for line in safety {
        html.push_str(&format!("<li>{line}</li>"));
    }
    html.push_str("</ul>");

    html.push_str("<h2>");
    html.push_str(if lang == Lang::Ru { "Лицензия" } else { "License" });
    html.push_str("</h2><ul>");
    for line in license {
        html.push_str(&format!("<li>{line}</li>"));
    }
    html.push_str("</ul>");

    html.push_str("<div class=\"note\">");
    html.push_str(if lang == Lang::Ru {
        "Справка сгенерирована ChipClean и открыта локально. Данные не покидают компьютер."
    } else {
        "This guide was generated by ChipClean and opened locally. No data leaves your computer."
    });
    html.push_str("</div></body></html>");

    html
}
