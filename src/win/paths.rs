use std::path::PathBuf;

use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::PWSTR;

pub fn downloads() -> Option<PathBuf> {
    let raw: PWSTR =
        unsafe { SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None) }.ok()?;
    let text = unsafe { raw.to_string() }.ok()?;
    unsafe {
        CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void));
    }

    let path = PathBuf::from(text);
    path.is_dir().then_some(path)
}
