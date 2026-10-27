use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CategoryId {
    TempUser,
    TempWindows,
    Thumbnails,
    WerReports,
    CrashDumps,
    CbsLogs,
    UpdateLogs,
    DeliveryOptimization,
    UpdateCache,
    WindowsOld,
    EdgeCache,
    ChromeCache,
    FirefoxCache,
    NvidiaDx,
    NvidiaGl,
    DirectXShader,
    RecycleBin,
    Downloads,
    Quarantine,
}

impl CategoryId {
    pub fn key(self) -> &'static str {
        match self {
            Self::TempUser => "temp_user",
            Self::TempWindows => "temp_windows",
            Self::Thumbnails => "thumbnails",
            Self::WerReports => "wer_reports",
            Self::CrashDumps => "crash_dumps",
            Self::CbsLogs => "cbs_logs",
            Self::UpdateLogs => "update_logs",
            Self::DeliveryOptimization => "delivery_optimization",
            Self::UpdateCache => "update_cache",
            Self::WindowsOld => "windows_old",
            Self::EdgeCache => "edge_cache",
            Self::ChromeCache => "chrome_cache",
            Self::FirefoxCache => "firefox_cache",
            Self::NvidiaDx => "nvidia_dx",
            Self::NvidiaGl => "nvidia_gl",
            Self::DirectXShader => "directx_shader",
            Self::RecycleBin => "recycle_bin",
            Self::Downloads => "downloads",
            Self::Quarantine => "quarantine",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    System,
    Browsers,
    Personal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Safe,
    Caution,
    Advanced,
}

#[derive(Debug, Clone)]
pub struct CleanupCategory {
    pub id: CategoryId,
    pub title: &'static str,
    pub summary: &'static str,
    pub icon: &'static str,
    pub group: Group,
    pub risk: Risk,
    pub requires_admin: bool,
    pub extra_confirmation: bool,
    pub default_enabled: bool,
    pub max_only: bool,
    pub targets: Vec<ScanTarget>,
}

#[derive(Debug, Clone)]
pub enum ScanTarget {
    Path {
        source: PathSource,
        min_age_days: u32,
        max_depth: usize,
    },
    RecycleBin,
    ChipCleanQuarantine,
}

impl ScanTarget {
    pub fn path(source: PathSource) -> Self {
        Self::Path {
            source,
            min_age_days: 0,
            max_depth: 0,
        }
    }

    pub fn shallow(source: PathSource) -> Self {
        Self::Path {
            source,
            min_age_days: 0,
            max_depth: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PathSource {
    Env {
        var: &'static str,
        sub: &'static str,
    },
    Windows {
        sub: &'static str,
    },
    ProgramData {
        sub: &'static str,
    },
    SystemDrive {
        sub: &'static str,
    },
    UserTemp,
    DownloadFolder,
    #[cfg(test)]
    Literal(PathBuf),
}

impl PathSource {
    pub fn resolve(&self) -> Vec<PathBuf> {
        let path = match self {
            Self::Env { var, sub } => std::env::var_os(var).map(|root| {
                let mut path = PathBuf::from(root);
                push_sub(&mut path, sub);
                path
            }),
            Self::Windows { sub } => Some(join_sub(windows_dir(), sub)),
            Self::ProgramData { sub } => Some(join_sub(program_data_dir(), sub)),
            Self::SystemDrive { sub } => Some(join_sub(system_drive(), sub)),
            Self::UserTemp => Some(std::env::temp_dir()),
            Self::DownloadFolder => crate::win::paths::downloads(),
            #[cfg(test)]
            Self::Literal(path) => Some(path.clone()),
        };

        path.map(expand_path).unwrap_or_default()
    }
}

fn windows_dir() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

fn program_data_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

fn system_drive() -> PathBuf {
    std::env::var_os("SystemDrive")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:"))
}

fn join_sub(mut root: PathBuf, sub: &str) -> PathBuf {
    push_sub(&mut root, sub);
    root
}

fn push_sub(root: &mut PathBuf, sub: &str) {
    for part in sub.split('\\').filter(|part| !part.is_empty()) {
        root.push(part);
    }
}

fn expand_path(path: PathBuf) -> Vec<PathBuf> {
    let text = path.to_string_lossy();
    if text.contains('*') || text.contains('?') {
        let pattern = text.replace('\\', "/");
        return glob::glob(&pattern)
            .map(|entries| entries.filter_map(|entry| entry.ok()).collect())
            .unwrap_or_default();
    }

    if path.exists() {
        vec![path]
    } else {
        Vec::new()
    }
}
