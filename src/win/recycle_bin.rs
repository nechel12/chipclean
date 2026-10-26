use anyhow::{Context, Result};
use windows::Win32::UI::Shell::{
    SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW, SHQUERYRBINFO,
    SHQueryRecycleBinW,
};
use windows::core::PCWSTR;

#[derive(Debug, Clone, Copy, Default)]
pub struct RecycleBinInfo {
    pub items: u64,
    pub bytes: u64,
}

pub fn query() -> Result<RecycleBinInfo> {
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };

    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.context("SHQueryRecycleBinW")?;

    Ok(RecycleBinInfo {
        items: info.i64NumItems.max(0) as u64,
        bytes: info.i64Size.max(0) as u64,
    })
}

pub fn empty() -> Result<()> {
    unsafe {
        SHEmptyRecycleBinW(
            None,
            PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    }
    .context("SHEmptyRecycleBinW")
}
