use std::{
    io,
    mem::size_of,
    ptr::{null, null_mut},
    sync::mpsc::{SyncSender, sync_channel},
    thread::{self, JoinHandle},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
            NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
            Shell_NotifyIconGetRect, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
            DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetSystemMetrics,
            GetWindowLongPtrW, HICON, IMAGE_ICON, LR_SHARED, LoadImageW, MB_ICONINFORMATION, MB_OK,
            MF_STRING, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
            RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON, SetForegroundWindow, SetWindowLongPtrW,
            TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CANCELMODE,
            WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WM_NULL, WNDCLASSW, WS_POPUP,
        },
    },
};

use super::{open_browser, wide};

const CALLBACK: u32 = WM_APP + 1;
const ICON_ID: u32 = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const OPEN: usize = 1;
const QUIT: usize = 2;

type Quit = dyn Fn() -> Result<(), &'static str> + Send;

pub struct Tray {
    window: HWND,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    pub fn start(url: String, quit: impl Fn() -> Result<(), &'static str> + Send + 'static) -> io::Result<Self> {
        let (ready, result) = sync_channel(1);
        let thread = thread::Builder::new()
            .name("xfold-tray".into())
            .spawn(move || {
                if let Err(error) = run(url, Box::new(quit), &ready) {
                    let _ = ready.send(Err(error));
                }
            })?;
        let window = result.recv().map_err(io::Error::other)?? as HWND;
        Ok(Self { window, thread: Some(thread) })
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.window, WM_CANCELMODE, 0, 0);
            PostMessageW(self.window, WM_CLOSE, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct WindowState {
    url: String,
    quit: Box<Quit>,
    icon: HICON,
    taskbar_created: u32,
}

impl WindowState {
    fn icon_data(&self, window: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: CALLBACK,
            hIcon: self.icon,
            ..Default::default()
        };
        let tip = wide("XFold - Open conversion dashboard");
        data.szTip[..tip.len()].copy_from_slice(&tip);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        data
    }

    fn add_icon(&self, window: HWND) {
        let data = self.icon_data(window);
        unsafe {
            if Shell_NotifyIconW(NIM_ADD, &data) != 0 {
                Shell_NotifyIconW(NIM_SETVERSION, &data);
            }
        }
    }

    fn open(&self, window: HWND) {
        if let Err(error) = open_browser(&self.url) {
            message(window, &error.to_string());
        }
    }
}

fn run(url: String, quit: Box<Quit>, ready: &SyncSender<io::Result<isize>>) -> io::Result<()> {
    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(io::Error::last_os_error());
    }
    let icon = unsafe {
        LoadImageW(instance, 1_usize as *const u16, IMAGE_ICON,
                   GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_SHARED)
    };
    if icon.is_null() {
        return Err(io::Error::last_os_error());
    }
    let class = wide("XFoldTrayWindow");
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let window = unsafe {
        CreateWindowExW(0, class.as_ptr(), null(), WS_POPUP, 0, 0, 0, 0,
                        null_mut(), null_mut(), instance, null())
    };
    if window.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut state = Box::new(WindowState {
        url,
        quit,
        icon,
        taskbar_created: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
    });
    // The window and all callbacks stay on this thread. Keep the state alive until destruction.
    unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, (&mut *state as *mut WindowState) as isize) };
    state.add_icon(window);
    let _ = ready.send(Ok(window as isize));
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    unsafe { DestroyWindow(window) };
    Ok(())
}

extern "system" fn window_proc(window: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let state = unsafe { (GetWindowLongPtrW(window, GWLP_USERDATA) as *const WindowState).as_ref() };
    if let Some(state) = state {
        if msg != 0 && msg == state.taskbar_created {
            state.add_icon(window);
            return 0;
        }
        match msg {
            CALLBACK => match (lparam & 0xffff) as u32 {
                NIN_SELECT | NIN_KEYSELECT => state.open(window),
                WM_CONTEXTMENU => menu(window),
                _ => (),
            },
            WM_COMMAND => match wparam & 0xffff {
                OPEN => state.open(window),
                QUIT => {
                    if let Err(reason) = (state.quit)() {
                        message(window, reason);
                    }
                }
                _ => (),
            },
            WM_DESTROY => unsafe {
                Shell_NotifyIconW(NIM_DELETE, &state.icon_data(window));
                PostQuitMessage(0);
            },
            _ => return unsafe { DefWindowProcW(window, msg, wparam, lparam) },
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, msg, wparam, lparam) }
}

fn menu(window: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(menu, MF_STRING, OPEN, wide("Open XFold").as_ptr());
        AppendMenuW(menu, MF_STRING, QUIT, wide("Quit XFold").as_ptr());
        let icon = NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: window,
            uID: ICON_ID,
            ..Default::default()
        };
        let mut bounds = RECT::default();
        let mut origin = POINT::default();
        if Shell_NotifyIconGetRect(&icon, &mut bounds) >= 0 {
            origin.x = bounds.left;
            origin.y = bounds.top;
        } else {
            GetCursorPos(&mut origin);
        }
        SetForegroundWindow(window);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, origin.x, origin.y, 0, window, null());
        PostMessageW(window, WM_NULL, 0, 0);
        DestroyMenu(menu);
    }
}

fn message(window: HWND, body: &str) {
    unsafe { MessageBoxW(window, wide(body).as_ptr(), wide("XFold").as_ptr(), MB_OK | MB_ICONINFORMATION) };
}
