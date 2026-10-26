use egui::Ui;
use egui_phosphor::regular as ph;

use crate::{app::Page, ui::common};

pub fn show(ui: &mut Ui) {
    common::page_header(ui, Page::Startup);

    common::empty_state(
        ui,
        ph::ROCKET_LAUNCH,
        "Менеджер автозагрузки",
        "Просмотр и отключение программ, которые стартуют вместе с Windows.",
        &[
            "Записи HKCU и HKLM: Run и RunOnce",
            "Пользовательская и общая папки «Автозагрузка»",
            "Задачи планировщика и службы",
            "Отключение без удаления записи",
        ],
    );
}
