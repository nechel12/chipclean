//! Локализация интерфейса: русский / английский.
//!
//! Язык хранится в настройках (`Settings.lang`) и устанавливается при старте
//! через `set_lang`. Все UI-строки читают текущий язык через `current()`.

use crate::app::Page;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    pub fn from_code(code: &str) -> Self {
        if code.eq_ignore_ascii_case("en") {
            Self::En
        } else {
            Self::Ru
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Ru => "ru",
            Self::En => "en",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ru => "Русский",
            Self::En => "English",
        }
    }
}

static CURRENT_LANG: std::sync::Mutex<Lang> = std::sync::Mutex::new(Lang::Ru);

/// Устанавливает текущий язык (используется при старте и смене в настройках).
pub fn set_lang_force(lang: Lang) {
    *CURRENT_LANG
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = lang;
}

/// Текущий язык интерфейса.
pub fn current() -> Lang {
    *CURRENT_LANG
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Короткий перевод ключевой фразы.
pub fn t(key: &str) -> &'static str {
    match (current(), key) {
        (Lang::Ru, "scan") => "Сканировать",
        (Lang::En, "scan") => "Scan",
        (Lang::Ru, "apply") => "Применить",
        (Lang::En, "apply") => "Apply",
        (Lang::Ru, "cancel") => "Отмена",
        (Lang::En, "cancel") => "Cancel",
        (Lang::Ru, "open") => "Открыть",
        (Lang::En, "open") => "Open",
        (Lang::Ru, "clean") => "Почистить",
        (Lang::En, "clean") => "Clean",
        (Lang::Ru, "refresh") => "Обновить",
        (Lang::En, "refresh") => "Refresh",
        (Lang::Ru, "find_traces") => "Найти следы",
        (Lang::En, "find_traces") => "Find traces",
        (Lang::Ru, "restore") => "Восстановить",
        (Lang::En, "restore") => "Restore",
        (Lang::Ru, "settings") => "Настройки",
        (Lang::En, "settings") => "Settings",
        (Lang::Ru, "help") => "Справка",
        (Lang::En, "help") => "Help",
        (Lang::Ru, "discard") => "Отменить изменения",
        (Lang::En, "discard") => "Discard changes",
        (Lang::Ru, "update_db") => "Скачать базу из интернета",
        (Lang::En, "update_db") => "Download database from the internet",
        (Lang::Ru, "restore_point") => "Точка восстановления",
        (Lang::En, "restore_point") => "Restore point",
        (Lang::Ru, "open_guide") => "Открыть руководство в браузере",
        (Lang::En, "open_guide") => "Open guide in browser",
        (Lang::Ru, "express_check") => "Экспресс-проверка",
        (Lang::En, "express_check") => "Express check",
        (Lang::Ru, "disk_analysis") => "Анализ диска",
        (Lang::En, "disk_analysis") => "Disk analysis",
        (Lang::Ru, "tweaks") => "Твики",
        (Lang::En, "tweaks") => "Tweaks",
        (Lang::Ru, "programs") => "Программы",
        (Lang::En, "programs") => "Programs",
        (Lang::Ru, "registry") => "Реестр",
        (Lang::En, "registry") => "Registry",
        (Lang::Ru, "cleanup") => "Очистка",
        (Lang::En, "cleanup") => "Cleanup",
        (Lang::Ru, "privacy") => "Личные данные",
        (Lang::En, "privacy") => "Privacy",
        (Lang::Ru, "startup") => "Автозагрузка",
        (Lang::En, "startup") => "Startup",
        (Lang::Ru, "traces") => "Следы",
        (Lang::En, "traces") => "Traces",
        (Lang::Ru, "snapshots") => "Снимки реестра",
        (Lang::En, "snapshots") => "Registry snapshots",
        (Lang::Ru, "rollback") => "Центр отката",
        (Lang::En, "rollback") => "Rollback center",
        (Lang::Ru, "editor") => "Редактор реестра",
        (Lang::En, "editor") => "Registry editor",
        (Lang::Ru, "community") => "Чистка сообществом",
        (Lang::En, "community") => "Community cleanup",
        (Lang::Ru, "status") => "Статус",
        (Lang::En, "status") => "Status",
        (Lang::Ru, "size") => "Размер",
        (Lang::En, "size") => "Size",
        (Lang::Ru, "language") => "Язык интерфейса",
        (Lang::En, "language") => "Interface language",
        (Lang::Ru, "beta_note") => "Часть технических сообщений пока только на русском.",
        (Lang::En, "beta_note") => "Some technical messages are still in Russian.",
        _ => "",
    }
}

impl Page {
    pub fn label(self) -> &'static str {
        match (current(), self) {
            (_, Page::Cleanup) => t("cleanup"),
            (_, Page::Registry) => t("registry"),
            (_, Page::RegistryEditor) => t("editor"),
            (_, Page::Snapshots) => t("snapshots"),
            (_, Page::Rollback) => t("rollback"),
            (_, Page::Privacy) => t("privacy"),
            (_, Page::Community) => t("community"),
            (_, Page::Express) => t("express_check"),
            (_, Page::DiskAnalysis) => t("disk_analysis"),
            (_, Page::Startup) => t("startup"),
            (_, Page::Traces) => t("traces"),
            (_, Page::Tweaks) => t("tweaks"),
            (_, Page::Programs) => t("programs"),
            (_, Page::Settings) => t("settings"),
            (_, Page::Help) => t("help"),
        }
    }

    pub fn title(self) -> &'static str {
        match (current(), self) {
            (Lang::Ru, Page::Cleanup) => "Очистка системы",
            (Lang::En, Page::Cleanup) => "System Cleanup",
            (Lang::Ru, Page::Registry) => "Реестр",
            (Lang::En, Page::Registry) => "Registry",
            (Lang::Ru, Page::RegistryEditor) => "Редактор реестра",
            (Lang::En, Page::RegistryEditor) => "Registry Editor",
            (Lang::Ru, Page::Snapshots) => "Снимки реестра",
            (Lang::En, Page::Snapshots) => "Registry Snapshots",
            (Lang::Ru, Page::Rollback) => "Центр отката",
            (Lang::En, Page::Rollback) => "Rollback Center",
            (Lang::Ru, Page::Privacy) => "Чистка личных данных",
            (Lang::En, Page::Privacy) => "Privacy Cleanup",
            (Lang::Ru, Page::Community) => "Чистка сообществом",
            (Lang::En, Page::Community) => "Community Cleanup",
            (Lang::Ru, Page::Express) => "Единая проверка системы",
            (Lang::En, Page::Express) => "Express Check",
            (Lang::Ru, Page::DiskAnalysis) => "Анализ диска",
            (Lang::En, Page::DiskAnalysis) => "Disk Analysis",
            (Lang::Ru, Page::Startup) => "Автозагрузка",
            (Lang::En, Page::Startup) => "Startup",
            (Lang::Ru, Page::Traces) => "Следы программ",
            (Lang::En, Page::Traces) => "Program Traces",
            (Lang::Ru, Page::Tweaks) => "Настройки системы",
            (Lang::En, Page::Tweaks) => "System Tweaks",
            (Lang::Ru, Page::Programs) => "Программы",
            (Lang::En, Page::Programs) => "Programs",
            (Lang::Ru, Page::Settings) => "Настройки",
            (Lang::En, Page::Settings) => "Settings",
            (Lang::Ru, Page::Help) => "Справка",
            (Lang::En, Page::Help) => "Help",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match (current(), self) {
            (Lang::Ru, Page::Cleanup) => {
                "Сканирование, предпросмотр и безопасное удаление с подтверждением."
            }
            (Lang::En, Page::Cleanup) => "Scan, preview and safely delete with confirmation.",
            (Lang::Ru, Page::Registry) => {
                "Безопасная проверка реестра и обязательный .reg-бэкап перед изменениями."
            }
            (Lang::En, Page::Registry) => {
                "Safe registry check with a mandatory .reg backup before changes."
            }
            (Lang::Ru, Page::RegistryEditor) => {
                "Просмотр, поиск и изменение реестра с .reg-бэкапами."
            }
            (Lang::En, Page::RegistryEditor) => {
                "Browse, search and edit the registry with .reg backups."
            }
            (Lang::Ru, Page::Snapshots) => "Создание снимков и сравнение изменений реестра.",
            (Lang::En, Page::Snapshots) => "Create snapshots and compare registry changes.",
            (Lang::Ru, Page::Rollback) => {
                "Восстановление реестра, карантина следов и отключённой автозагрузки."
            }
            (Lang::En, Page::Rollback) => {
                "Restore registry, quarantined traces and disabled startup items."
            }
            (Lang::Ru, Page::Privacy) => "История браузеров, списки файлов и недавние документы.",
            (Lang::En, Page::Privacy) => "Browser history, file lists and recent documents.",
            (Lang::Ru, Page::Community) => "Правила очистки из открытой базы winapp2.ini.",
            (Lang::En, Page::Community) => "Cleanup rules from the open winapp2.ini database.",
            (Lang::Ru, Page::Express) => {
                "Скан всех групп одной кнопкой: система, личные данные, автозагрузка, твики."
            }
            (Lang::En, Page::Express) => {
                "Scan all groups with one button: system, privacy, startup, tweaks."
            }
            (Lang::Ru, Page::DiskAnalysis) => {
                "Поиск крупнейших файлов и оценка занятого места на выбранном томе."
            }
            (Lang::En, Page::DiskAnalysis) => {
                "Find largest files and estimate disk usage on a selected volume."
            }
            (Lang::Ru, Page::Startup) => {
                "Run/RunOnce, папки «Автозагрузка», планировщик задач и службы."
            }
            (Lang::En, Page::Startup) => {
                "Run/RunOnce, startup folders, Task Scheduler and services."
            }
            (Lang::Ru, Page::Traces) => "Битые ярлыки и следы программ: карантин или удаление.",
            (Lang::En, Page::Traces) => {
                "Broken shortcuts and program traces: quarantine or delete."
            }
            (Lang::Ru, Page::Tweaks) => {
                "Скрытые настройки Windows: включение/отключение по образцу Reg Organizer."
            }
            (Lang::En, Page::Tweaks) => {
                "Hidden Windows settings: enable/disable following Reg Organizer."
            }
            (Lang::Ru, Page::Programs) => {
                "Установленные программы и запуск их штатных деинсталляторов."
            }
            (Lang::En, Page::Programs) => {
                "Installed programs and running their official uninstallers."
            }
            (Lang::Ru, Page::Settings) => {
                "Правила очистки, расположение журналов и сведения о сборке."
            }
            (Lang::En, Page::Settings) => "Cleanup rules, log locations and build information.",
            (Lang::Ru, Page::Help) => "Краткое описание возможностей и гарантий безопасности.",
            (Lang::En, Page::Help) => "Overview of features and safety guarantees.",
        }
    }
}
