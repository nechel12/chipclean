use egui::{
    Align2, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    core::model::Risk,
    theme::*,
    ui::{common, widgets},
};

struct RegistryIssue {
    icon: &'static str,
    title: &'static str,
    summary: &'static str,
    risk: Risk,
}

const ISSUES: &[RegistryIssue] = &[
    RegistryIssue {
        icon: ph::DATABASE,
        title: "Неверные записи деинсталляции",
        summary: "Осиротевшие ключи Uninstall",
        risk: Risk::Advanced,
    },
    RegistryIssue {
        icon: ph::ROCKET_LAUNCH,
        title: "Неверные записи автозагрузки",
        summary: "Run и RunOnce с отсутствующими файлами",
        risk: Risk::Caution,
    },
    RegistryIssue {
        icon: ph::LINK_SIMPLE_BREAK,
        title: "Неверные ярлыки",
        summary: "Ссылки на удалённые файлы",
        risk: Risk::Caution,
    },
    RegistryIssue {
        icon: ph::PATH,
        title: "Неверные пути в реестре",
        summary: "Пути к несуществующим файлам",
        risk: Risk::Advanced,
    },
    RegistryIssue {
        icon: ph::MAGNIFYING_GLASS,
        title: "Глубокий поиск путей",
        summary: "Больше веток, больше времени",
        risk: Risk::Advanced,
    },
    RegistryIssue {
        icon: ph::TREE_STRUCTURE,
        title: "Неверные объекты реестра",
        summary: "Битые значения и ссылки",
        risk: Risk::Advanced,
    },
    RegistryIssue {
        icon: ph::USER,
        title: "Пути в профилях пользователей",
        summary: "HKCU для всех профилей",
        risk: Risk::Advanced,
    },
    RegistryIssue {
        icon: ph::APP_WINDOW,
        title: "App Paths и типы документов",
        summary: "Регистрация запуска и ассоциаций",
        risk: Risk::Caution,
    },
];

pub fn show(ui: &mut Ui) {
    common::page_header(ui, Page::Registry);

    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(11), ACCENT_SOFT);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    ph::SHIELD_CHECK,
                    FontId::new(22.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Очистка реестра с резервной копией")
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Перед любыми изменениями ChipClean экспортирует затронутые ключи в .reg-файл. Удаление будет доступно только после ручного просмотра найденного.",
                        )
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "Следующий этап", WARNING);
                    },
                );
            });
        });

    ui.add_space(16.0);
    ui.label(
        RichText::new("Что проверим")
            .font(font_semibold(12.0))
            .color(TEXT),
    );
    ui.add_space(2.0);
    ui.label(
        RichText::new("Безопасное подмножество проблем, без агрессивных эвристик.")
            .font(font(9.5))
            .color(TEXT_DIM),
    );
    ui.add_space(8.0);

    ui.columns(2, |columns| {
        for (index, issue) in ISSUES.iter().enumerate() {
            let column = &mut columns[index % 2];
            widgets::planned_row(column, issue.icon, issue.title, issue.summary, issue.risk);
            column.add_space(8.0);
        }
    });
}
