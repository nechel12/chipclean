use egui::{CornerRadius, Frame, Margin, RichText, Stroke, Ui};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    i18n::{Lang, current, t},
    theme::*,
    ui::{common, widgets},
};

pub fn show(ui: &mut Ui) {
    common::page_header(ui, Page::Help);

    let lang = current();

    let (what_title, what_lines) = match lang {
        Lang::Ru => (
            "Что умеет ChipClean",
            vec![
                "Очистка системы: временные файлы, кэши, логи, отчёты об ошибках, обновления Windows, корзина, загрузки.",
                "Реестр: безопасная проверка, редактор с поиском и заменой, импорт .reg с предпросмотром, снимки и откат.",
                "Автозагрузка: Run/RunOnce, папки автозагрузки, задачи планировщика, службы — с отключением и карантином.",
                "Личные данные: история браузеров, недавние документы и списки файлов.",
                "Чистка сообществом: правила winapp2.ini с обновлением базы из интернета.",
                "Твики: скрытые настройки Windows по образцу Reg Organizer, с резервными копиями .reg.",
                "Фоновая очистка: ежедневная задача и проверка при входе через Планировщик.",
                "Экспресс-проверка: один запуск всех сканов.",
                "Анализ диска: крупнейшие файлы и занятость тома.",
            ],
        ),
        Lang::En => (
            "What ChipClean does",
            vec![
                "System cleanup: temporary files, caches, logs, error reports, Windows updates, Recycle Bin, downloads.",
                "Registry: safe check, editor with search and replace, .reg import with preview, snapshots and rollback.",
                "Startup: Run/RunOnce, startup folders, Task Scheduler tasks, services — disable with quarantine.",
                "Privacy: browser history, recent documents and file lists.",
                "Community cleanup: winapp2.ini rules with online database updates.",
                "Tweaks: hidden Windows settings following Reg Organizer, with .reg backups.",
                "Background cleanup: daily task and logon check via Task Scheduler.",
                "Express check: run all scans with one button.",
                "Disk analysis: largest files and volume usage.",
            ],
        ),
    };

    let (safety_title, safety_lines) = match lang {
        Lang::Ru => (
            "Гарантии безопасности",
            vec![
                "Все изменения реестра и файлов пишутся в журнал операций.",
                "Перед изменением значений твиков создаётся резервная копия .reg.",
                "Удаление только после предпросмотра и подтверждения.",
                "Программа не отправляет данные наружу; база правил скачивается только по явной команде.",
            ],
        ),
        Lang::En => (
            "Safety guarantees",
            vec![
                "All registry and file changes are written to the operations journal.",
                "A .reg backup is created before applying tweak values.",
                "Deletion only after preview and confirmation.",
                "The program does not send data anywhere; the rules database is downloaded only on explicit command.",
            ],
        ),
    };

    let (license_title, license_lines) = match lang {
        Lang::Ru => (
            "Лицензия",
            vec![
                "ChipClean — свободная программа (Apache-2.0).",
                "Твики и совместимые правила перенесены из открытого конфигурационного файла Reg Organizer (ChemTable Software).",
                "Используйте на свой риск; проверяйте выбранные секции очистки.",
            ],
        ),
        Lang::En => (
            "License",
            vec![
                "ChipClean is free software (Apache-2.0).",
                "Tweaks and compatible rules were ported from the open configuration file of Reg Organizer (ChemTable Software).",
                "Use at your own risk; review selected cleanup sections.",
            ],
        ),
    };

    card(ui, what_title, &what_lines);
    card(ui, safety_title, &safety_lines);
    card(ui, license_title, &license_lines);

    ui.add_space(6.0);
    if widgets::secondary_button_icon(ui, t("open_guide"), ph::BOOK_OPEN).clicked() {
        if let Err(err) = crate::core::guide::open_guide() {
            tracing::warn!(%err, "failed to open guide");
        }
    }
}

fn card(ui: &mut Ui, title: &str, lines: &[&str]) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.label(RichText::new(title).font(font_semibold(13.0)).color(TEXT));
            ui.add_space(10.0);
            for line in lines {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("•").font(font(11.0)).color(ACCENT));
                    ui.label(RichText::new(*line).font(font(10.5)).color(TEXT_MUTED));
                });
                ui.add_space(4.0);
            }
        });
    ui.add_space(12.0);
}
