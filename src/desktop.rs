use std::{
    ffi::OsString,
    io,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
    UI::{
        Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER,
            OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_NODEREFERENCELINKS,
            OFN_PATHMUSTEXIST, OPENFILENAMEW,
        },
        Shell::ShellExecuteW,
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

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

pub fn pick_file(extension: &str, save: bool) -> io::Result<Option<PathBuf>> {
    let filter = if save {
        let label = match extension {
            "dts" => "DTS-HD Master Audio",
            "mka" => "Matroska audio",
            "mkv" => "Matroska video",
            _ => "TrueHD audio",
        };
        wide(&format!("{label} (*.{extension})\0*.{extension}\0"))
    } else {
        wide("DTS:X sources (*.dts;*.mkv)\0*.dts;*.mkv\0")
    };
    let title = wide(if save {
        "Choose export destination"
    } else {
        "Choose a DTS:X source"
    });
    let extension = wide(extension);
    let mut filename = vec![0_u16; 32_768];
    let mut dialog: OPENFILENAMEW = unsafe { zeroed() };
    dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.nFilterIndex = 1;
    dialog.lpstrFile = filename.as_mut_ptr();
    dialog.nMaxFile = filename.len() as u32;
    dialog.lpstrTitle = title.as_ptr();
    if save {
        dialog.lpstrDefExt = extension.as_ptr();
    }
    dialog.Flags = OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST | OFN_NODEREFERENCELINKS;
    let selected = unsafe {
        if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            dialog.Flags |= OFN_FILEMUSTEXIST | OFN_HIDEREADONLY;
            GetOpenFileNameW(&mut dialog)
        }
    };
    if selected == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 {
            Ok(None)
        } else {
            Err(io::Error::other(format!(
                "Windows file picker failed ({error:#x})."
            )))
        };
    }
    let end = filename
        .iter()
        .position(|&ch| ch == 0)
        .unwrap_or(filename.len());
    Ok(Some(PathBuf::from(OsString::from_wide(&filename[..end]))))
}
