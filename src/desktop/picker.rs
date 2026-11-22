use std::{
    ffi::{OsStr, OsString, c_void},
    io,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{NonNull, null, null_mut},
    slice, thread,
    time::{Duration, Instant},
};

use windows_sys::{
    Win32::{
        Foundation::HWND,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
        },
        UI::{
            Shell::{
                FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_NODEREFERENCELINKS,
                FOS_NOREADONLYRETURN, FOS_PATHMUSTEXIST, FileOpenDialog, FileSaveDialog,
                SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW},
        },
    },
    core::{GUID, HRESULT, PCWSTR, PWSTR},
};

const IFILEDIALOG: GUID = GUID::from_u128(0x42f85136_db7e_439c_85f1_e4075d135fc8);
const ISHELLITEM: GUID = GUID::from_u128(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);
const CANCELLED: HRESULT = 0x800704c7_u32 as HRESULT;

#[repr(C)]
struct Filter {
    name: PCWSTR,
    pattern: PCWSTR,
}

// Windows SDK interface layout, including the inherited IUnknown and IModalWindow slots.
// The reserved fields cover methods XFold never calls.
#[repr(C)]
struct DialogVtable {
    _unknown: [usize; 3],
    show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    set_file_types: unsafe extern "system" fn(*mut c_void, u32, *const Filter) -> HRESULT,
    _file_type_and_events: [usize; 4],
    set_options: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    _get_options_and_default_folder: [usize; 2],
    set_folder: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    _get_folder_and_selection: [usize; 2],
    set_file_name: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    _get_file_name: usize,
    set_title: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    _set_button_and_field_labels: [usize; 2],
    get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    _add_place: usize,
    set_default_extension: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
}

#[repr(C)]
struct ShellItemVtable {
    _unknown_and_binding: [usize; 5],
    get_display_name: unsafe extern "system" fn(*mut c_void, i32, *mut PWSTR) -> HRESULT,
}

#[repr(C)]
struct UnknownVtable {
    _query_and_add_ref: [usize; 2],
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

struct ComObject(NonNull<c_void>);

impl ComObject {
    fn get(result: HRESULT, object: *mut c_void) -> io::Result<Self> {
        check(result)?;
        NonNull::new(object)
            .map(Self)
            .ok_or_else(|| io::Error::other("Windows returned no dialog object."))
    }

    fn ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    // The interface returned by the Windows API must match V.
    unsafe fn vtable<V>(&self) -> &V {
        unsafe { &**self.ptr().cast::<*const V>() }
    }
}

impl Drop for ComObject {
    fn drop(&mut self) {
        unsafe { (self.vtable::<UnknownVtable>().release)(self.ptr()) };
    }
}

struct Apartment;

impl Apartment {
    fn enter() -> io::Result<Self> {
        check(unsafe {
            CoInitializeEx(
                null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        })?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn check(result: HRESULT) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::other(format!(
            "Windows file dialog failed ({:#010x}).",
            result as u32
        )))
    } else {
        Ok(())
    }
}

pub fn valid_marker(marker: &str) -> bool {
    marker.len() == 32 && marker.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn foreground_browser(marker: &str) -> io::Result<HWND> {
    let caption = format!("XFold {marker}");
    let deadline = Instant::now() + Duration::from_millis(1250);
    let mut buffer = [0u16; 512];
    loop {
        let window = unsafe { GetForegroundWindow() };
        if !window.is_null() {
            let len = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
            if len > 0 && String::from_utf16_lossy(&buffer[..len as usize]).contains(&caption) {
                return Ok(window);
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "Keep the XFold browser tab in front and click the picker button again.",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn pick_file(
    marker: &str,
    extension: &str,
    save: bool,
    directory: Option<&Path>,
    suggestion: Option<&OsStr>,
) -> io::Result<Option<PathBuf>> {
    let owner = foreground_browser(marker)?;
    let _apartment = Apartment::enter()?;
    let dialog = configure(extension, save, directory, suggestion)?;
    if unsafe { GetForegroundWindow() } != owner {
        return Err(io::Error::other(
            "Keep the XFold browser tab in front and click the picker button again.",
        ));
    }
    let methods = unsafe { dialog.vtable::<DialogVtable>() };
    let result = unsafe { (methods.show)(dialog.ptr(), owner) };
    if result == CANCELLED {
        return Ok(None);
    }
    check(result)?;
    let mut item = null_mut();
    let result = unsafe { (methods.get_result)(dialog.ptr(), &mut item) };
    let item = ComObject::get(result, item)?;
    let mut path = null_mut();
    check(unsafe {
        (item.vtable::<ShellItemVtable>().get_display_name)(
            item.ptr(),
            SIGDN_FILESYSPATH,
            &mut path,
        )
    })?;
    if path.is_null() {
        return Err(io::Error::other("Windows returned no file path."));
    }
    let result = unsafe {
        let mut len = 0;
        while *path.add(len) != 0 {
            len += 1;
        }
        let result = PathBuf::from(OsString::from_wide(slice::from_raw_parts(path, len)));
        CoTaskMemFree(path.cast());
        result
    };
    Ok(Some(result))
}

fn configure(
    extension: &str,
    save: bool,
    directory: Option<&Path>,
    suggestion: Option<&OsStr>,
) -> io::Result<ComObject> {
    let class = if save {
        &FileSaveDialog
    } else {
        &FileOpenDialog
    };
    let mut object = null_mut();
    let result = unsafe {
        CoCreateInstance(
            class,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IFILEDIALOG,
            &mut object,
        )
    };
    let dialog = ComObject::get(result, object)?;
    let methods = unsafe { dialog.vtable::<DialogVtable>() };
    let ptr = dialog.ptr();
    let options = FOS_FORCEFILESYSTEM
        | FOS_NOCHANGEDIR
        | FOS_PATHMUSTEXIST
        | FOS_NODEREFERENCELINKS
        | if save {
            FOS_NOREADONLYRETURN
        } else {
            FOS_FILEMUSTEXIST
        };
    let (title, label, pattern) = if save {
        (
            "Save XFold export",
            format!("XFold export (*.{extension})"),
            format!("*.{extension}"),
        )
    } else {
        (
            "Select XFold source",
            "DTS:X sources (*.mkv;*.dts)".to_owned(),
            "*.mkv;*.dts".to_owned(),
        )
    };
    let label = super::wide(&label);
    let pattern = super::wide(&pattern);
    let filter = Filter {
        name: label.as_ptr(),
        pattern: pattern.as_ptr(),
    };
    unsafe {
        check((methods.set_options)(ptr, options))?;
        check((methods.set_title)(ptr, super::wide(title).as_ptr()))?;
        check((methods.set_file_types)(ptr, 1, &filter))?;
        if save {
            check((methods.set_default_extension)(
                ptr,
                super::wide(extension).as_ptr(),
            ))?;
        }
        if let Some(name) = suggestion {
            let name: Vec<u16> = name.encode_wide().chain([0]).collect();
            check((methods.set_file_name)(ptr, name.as_ptr()))?;
        }
        if let Some(directory) = directory {
            let name = directory.to_string_lossy();
            let path = if let Some(unc) = name.strip_prefix(r"\\?\UNC\") {
                super::wide(&format!(r"\\{unc}"))
            } else {
                super::wide(name.strip_prefix(r"\\?\").unwrap_or(&name))
            };
            let mut folder = null_mut();
            let result =
                SHCreateItemFromParsingName(path.as_ptr(), null_mut(), &ISHELLITEM, &mut folder);
            let folder = ComObject::get(result, folder)?;
            check((methods.set_folder)(ptr, folder.ptr()))?;
        }
    }
    Ok(dialog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configures_both_windows_dialogs() {
        let _apartment = Apartment::enter().unwrap();
        configure("dts", false, None, None).unwrap();
        configure(
            "mka",
            true,
            Some(&std::env::current_dir().unwrap()),
            Some(OsStr::new("export.mka")),
        )
        .unwrap();
    }
}
