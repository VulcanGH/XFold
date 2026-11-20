//! Win32 operations with handle ownership and atomic process containment.
use std::{
    ffi::{OsStr, c_void},
    fs::File,
    io,
    mem::{size_of, zeroed},
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::Path,
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
};

type Handle = *mut c_void;
type Bool = i32;

#[repr(C)]
struct Startup {
    size: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    dimensions: [u32; 7],
    flags: u32,
    show: u16,
    reserved_size: u16,
    reserved_data: *mut u8,
    stdin: Handle,
    stdout: Handle,
    stderr: Handle,
    attributes: *mut c_void,
}

#[repr(C)]
struct ProcessInfo {
    process: Handle,
    thread: Handle,
    ids: [u32; 2],
}

#[repr(C)]
struct JobLimits {
    // JOBOBJECT_BASIC_LIMIT_INFORMATION, followed by IO_COUNTERS and SIZE_Ts.
    times: [i64; 2],
    flags: u32,
    working_sets: [usize; 2],
    active_process_limit: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
    io: [u64; 6],
    memory: [usize; 4],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CloseHandle(handle: Handle) -> Bool;
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: u32, data: *const c_void, len: u32) -> Bool;
    fn TerminateJobObject(job: Handle, code: u32) -> Bool;
    fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> Bool;
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        size: *mut usize,
    ) -> Bool;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        key: usize,
        value: *const c_void,
        size: usize,
        previous: *mut c_void,
        returned: *mut usize,
    ) -> Bool;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreateProcessW(
        app: *const u16,
        command: *mut u16,
        process_security: *const c_void,
        thread_security: *const c_void,
        inherit: Bool,
        flags: u32,
        env: *const c_void,
        cwd: *const u16,
        startup: *const Startup,
        info: *mut ProcessInfo,
    ) -> Bool;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> Bool;
    fn GetDiskFreeSpaceExW(
        path: *const u16,
        available: *mut u64,
        total: *mut u64,
        free: *mut u64,
    ) -> Bool;
    fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> Bool;
    fn SetConsoleCtrlHandler(handler: extern "system" fn(u32) -> Bool, add: Bool) -> Bool;
}

struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Attributes(Vec<usize>);
impl Attributes {
    fn new() -> io::Result<Self> {
        let mut bytes = 0;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes) };
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0; bytes.div_ceil(size_of::<usize>())];
        ok(unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut bytes)
        })?;
        Ok(Self(storage))
    }

    fn ptr(&mut self) -> *mut c_void {
        self.0.as_mut_ptr().cast()
    }

    fn set(&mut self, key: usize, handles: &[Handle]) -> io::Result<()> {
        ok(unsafe {
            UpdateProcThreadAttribute(
                self.ptr(),
                0,
                key,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
                null_mut(),
                null_mut(),
            )
        })
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.ptr()) };
    }
}

fn ok(result: Bool) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain([0]).collect()
}

// Quote for the Windows C-runtime argv parser, including backslashes before quotes.
fn append_arg(line: &mut Vec<u16>, arg: &OsStr) {
    const SLASH: u16 = b'\\' as u16;
    const QUOTE: u16 = b'"' as u16;
    line.push(QUOTE);
    let mut slashes = 0;
    for ch in arg.encode_wide() {
        if ch == SLASH {
            slashes += 1;
            continue;
        }
        let escaped = if ch == QUOTE {
            slashes * 2 + 1
        } else {
            slashes
        };
        line.extend(std::iter::repeat_n(SLASH, escaped));
        slashes = 0;
        line.push(ch);
    }
    line.extend(std::iter::repeat_n(SLASH, slashes * 2));
    line.push(QUOTE);
}

pub struct Child {
    job: OwnedHandle,
    process: OwnedHandle,
}

impl Child {
    pub fn spawn(
        exe: &Path,
        args: &[impl AsRef<OsStr>],
        cwd: &Path,
        stdout: &File,
        stderr: &File,
    ) -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(null(), null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = OwnedHandle(handle);
        let mut limits: JobLimits = unsafe { zeroed() };
        limits.flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        ok(unsafe {
            SetInformationJobObject(
                job.0,
                9,
                (&limits as *const JobLimits).cast(),
                size_of::<JobLimits>() as u32,
            )
        })?;

        let stdin = File::open("NUL")?;
        let handles = [
            stdin.as_raw_handle(),
            stdout.as_raw_handle(),
            stderr.as_raw_handle(),
        ];
        // Only this creation can inherit these three handles.
        let mut attributes = Attributes::new()?;
        let jobs = [job.0];
        for handle in handles {
            ok(unsafe { SetHandleInformation(handle, 1, 1) })?;
        }
        attributes.set(0x20002, &handles)?; // PROC_THREAD_ATTRIBUTE_HANDLE_LIST
        attributes.set(0x2000d, &jobs)?; // PROC_THREAD_ATTRIBUTE_JOB_LIST
        let mut startup: Startup = unsafe { zeroed() };
        startup.size = size_of::<Startup>() as u32;
        startup.flags = 0x100; // STARTF_USESTDHANDLES
        [startup.stdin, startup.stdout, startup.stderr] = handles;
        startup.attributes = attributes.ptr();
        let mut line = Vec::new();
        append_arg(&mut line, exe.as_os_str());
        for arg in args {
            line.push(32);
            append_arg(&mut line, arg.as_ref());
        }
        line.push(0);
        let mut info: ProcessInfo = unsafe { zeroed() };
        let result = ok(unsafe {
            CreateProcessW(
                wide(exe.as_os_str()).as_ptr(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                0x08080000,
                null(),
                wide(cwd.as_os_str()).as_ptr(),
                &startup,
                &mut info,
            )
        });
        for handle in handles {
            unsafe { SetHandleInformation(handle, 1, 0) };
        }
        result?;
        drop(OwnedHandle(info.thread));
        Ok(Self {
            job,
            process: OwnedHandle(info.process),
        })
    }

    pub fn poll(&self, milliseconds: u32) -> io::Result<Option<u32>> {
        match unsafe { WaitForSingleObject(self.process.0, milliseconds) } {
            0 => {
                let mut code = 0;
                ok(unsafe { GetExitCodeProcess(self.process.0, &mut code) })?;
                Ok(Some(code))
            }
            258 => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub fn terminate(&self) -> io::Result<()> {
        ok(unsafe { TerminateJobObject(self.job.0, 1) })?;
        self.poll(5_000)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "codec process did not exit"))?;
        Ok(())
    }
}

pub fn free_space(path: &Path) -> io::Result<u64> {
    let mut available = 0;
    ok(unsafe {
        GetDiskFreeSpaceExW(
            wide(path.as_os_str()).as_ptr(),
            &mut available,
            null_mut(),
            null_mut(),
        )
    })?;
    Ok(available)
}

pub fn rename(source: &Path, target: &Path, replace: bool) -> io::Result<()> {
    // WRITE_THROUGH, no cross-volume copy fallback. Only state snapshots may replace.
    let flags = 8 | u32::from(replace);
    ok(unsafe {
        MoveFileExW(
            wide(source.as_os_str()).as_ptr(),
            wide(target.as_os_str()).as_ptr(),
            flags,
        )
    })
}

pub static CANCELLED: AtomicBool = AtomicBool::new(false);

extern "system" fn console_control(event: u32) -> Bool {
    if matches!(event, 0 | 1) {
        CANCELLED.store(true, Ordering::Relaxed);
        1
    } else {
        0
    }
}

pub fn install_console_handler() -> io::Result<()> {
    ok(unsafe { SetConsoleCtrlHandler(console_control, 1) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: Bool, process_id: u32) -> Handle;
    }

    #[test]
    fn closing_job_kills_grandchild() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("job-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let stdout = File::create_new(root.join("stdout")).unwrap();
        let stderr = File::create_new(root.join("stderr")).unwrap();
        let system =
            std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let powershell = system.join("WindowsPowerShell/v1.0/powershell.exe");
        let script = "$child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32/ping.exe') -ArgumentList '127.0.0.1 -t' -WindowStyle Hidden -PassThru; Set-Content -LiteralPath child-id -Value $child.Id; Wait-Process -Id $child.Id";
        let child = Child::spawn(
            &powershell,
            &["-NoProfile", "-Command", script],
            &root,
            &stdout,
            &stderr,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Ok(text) = fs::read_to_string(root.join("child-id")) {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    break pid;
                }
            }
            assert!(Instant::now() < deadline, "grandchild PID was not reported");
            thread::sleep(Duration::from_millis(20));
        };
        let handle = unsafe { OpenProcess(0x00100000, 0, pid) }; // SYNCHRONIZE
        assert!(
            !handle.is_null(),
            "grandchild must be alive before closing its job"
        );
        let handle = OwnedHandle(handle);
        let Child { job, process } = child;
        drop(job);
        assert_eq!(unsafe { WaitForSingleObject(handle.0, 5_000) }, 0);
        assert_eq!(unsafe { WaitForSingleObject(process.0, 5_000) }, 0);
        drop(stdout);
        drop(stderr);
        for name in ["stdout", "stderr", "child-id"] {
            fs::remove_file(root.join(name)).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
}
