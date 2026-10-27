use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
    OPENFILENAMEW,
};
use windows::core::{PCWSTR, PWSTR};

pub fn open_reg_file() -> Option<PathBuf> {
    let mut buffer = vec![0_u16; 32_768];
    let filter = crate::win::wide("Файлы реестра (*.reg)\0*.reg\0Все файлы (*.*)\0*.*\0");
    let title = crate::win::wide("Выберите файл .reg");
    let extension = crate::win::wide("reg");

    let mut options = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(extension.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER,
        ..Default::default()
    };

    let confirmed = unsafe { GetOpenFileNameW(&mut options) };
    if !confirmed.as_bool() {
        return None;
    }

    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(0);
    if length == 0 {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
}
