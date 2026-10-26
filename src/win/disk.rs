use anyhow::{Context, Result};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::core::PCWSTR;

#[derive(Debug, Clone, Copy)]
pub struct DiskSpace {
    pub free: u64,
    pub total: u64,
}

impl DiskSpace {
    pub fn used_fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            ((self.total.saturating_sub(self.free)) as f64 / self.total as f64) as f32
        }
    }
}

pub fn system_drive_space() -> Result<DiskSpace> {
    let root = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned()) + "\\";
    let root_w = crate::win::wide(&root);
    let mut free = 0_u64;
    let mut total = 0_u64;

    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(root_w.as_ptr()),
            Some(&mut free),
            Some(&mut total),
            None,
        )
    }
    .context("GetDiskFreeSpaceExW")?;

    Ok(DiskSpace { free, total })
}
