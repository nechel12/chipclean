pub mod dialog;
pub mod disk;
pub mod elevation;
pub mod file_dialog;
pub mod paths;
pub mod profile;
pub mod recycle_bin;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wide_os(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}
