use std::{
    io,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
    System::Console::{FreeConsole, GetConsoleProcessList},
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

mod picker;
pub use picker::{pick_file, valid_marker};
mod tray;
pub use tray::Tray;

pub fn detach_standalone_console() {
    let mut process = 0;
    // Leave a terminal shared with a caller attached for Ctrl+C and diagnostics.
    if unsafe { GetConsoleProcessList(&mut process, 1) } == 1 {
        unsafe { FreeConsole() };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

pub fn token() -> io::Result<String> {
    let mut bytes = [0_u8; 32];
    let result = unsafe {
        BCryptGenRandom(
            null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if result < 0 {
        return Err(io::Error::other(format!(
            "Windows random generator failed ({result:#x})"
        )));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn open_browser(url: &str) -> io::Result<()> {
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            wide(url).as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        return Err(io::Error::other(format!(
            "Windows could not open the browser ({result})."
        )));
    }
    Ok(())
}
