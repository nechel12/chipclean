use egui::Ui;
use egui_phosphor::regular as ph;

use crate::{app::Page, ui::common};

pub fn show(ui: &mut Ui) {
    common::page_header(ui, Page::Programs);

    common::empty_state(
        ui,
        ph::PACKAGE,
        "Удаление программ",
        "Список установленного ПО с запуском штатных деинсталляторов.",
        &[
            "Записи Uninstall для x64, x86 и текущего пользователя",
            "Поиск и сортировка по имени, издателю и размеру",
            "Запуск деинсталлятора с правами администратора",
            "Поиск остатков после удаления",
        ],
    );
}
