use anyhow::{Result, bail};
use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

pub fn is_elevated() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

pub fn relaunch_elevated() -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_w = crate::win::wide_os(exe.as_os_str());
    let verb_w = crate::win::wide("runas");

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(exe_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    let code = result.0 as isize;
    if code <= 32 {
        bail!("ShellExecuteW(runas) вернул код {code}");
    }

    Ok(())
}
