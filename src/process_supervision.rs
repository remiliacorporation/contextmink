use std::process::{Child, Command};

#[cfg(unix)]
use anyhow::Context as _;
use anyhow::{Result, anyhow};

#[cfg(unix)]
pub(crate) fn configure(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

#[cfg(windows)]
pub(crate) fn configure(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

    command.creation_flags(CREATE_SUSPENDED);
}

#[cfg(windows)]
pub(crate) struct ChildSupervisor(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for ChildSupervisor {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
pub(crate) fn supervise(child: &mut Child) -> Result<ChildSupervisor> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        let code = unsafe { GetLastError() };
        terminate_child(child);
        return Err(anyhow!(
            "failed to create child supervision job: win32 error {code}"
        ));
    }
    let mut limits = unsafe { std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    let assigned = configured != 0
        && unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) } != 0;
    if !assigned {
        let code = unsafe { GetLastError() };
        unsafe {
            CloseHandle(job);
        }
        terminate_child(child);
        return Err(anyhow!(
            "failed to attach child to kill-on-close supervision job: win32 error {code}"
        ));
    }
    if let Err(error) = resume_child(child) {
        unsafe {
            CloseHandle(job);
        }
        let _ = child.wait(); // guardrail: allow-ignore-result resume failure is already the authoritative error
        return Err(error);
    }
    Ok(ChildSupervisor(job))
}

#[cfg(windows)]
fn resume_child(child: &Child) -> Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        let code = unsafe { GetLastError() };
        return Err(anyhow!(
            "failed to enumerate suspended child threads: win32 error {code}"
        ));
    }
    let mut entry = unsafe { std::mem::zeroed::<THREADENTRY32>() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    let mut available = unsafe { Thread32First(snapshot, &mut entry) };
    while available != 0 {
        if entry.th32OwnerProcessID == child.id() {
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                let resumed = unsafe { ResumeThread(thread) };
                let code = if resumed == u32::MAX {
                    Some(unsafe { GetLastError() })
                } else {
                    None
                };
                unsafe {
                    CloseHandle(thread);
                    CloseHandle(snapshot);
                }
                if let Some(code) = code {
                    return Err(anyhow!("failed to resume child thread: win32 error {code}"));
                }
                return Ok(());
            }
        }
        available = unsafe { Thread32Next(snapshot, &mut entry) };
    }
    unsafe {
        CloseHandle(snapshot);
    }
    Err(anyhow!(
        "suspended child {} has no resumable primary thread",
        child.id()
    ))
}

#[cfg(unix)]
pub(crate) struct ChildSupervisor {
    release_fd: std::os::fd::RawFd,
    watchdog_pid: libc::pid_t,
}

#[cfg(unix)]
impl Drop for ChildSupervisor {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.release_fd);
            while libc::waitpid(self.watchdog_pid, std::ptr::null_mut(), 0) == -1 {
                if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn supervise(child: &mut Child) -> Result<ChildSupervisor> {
    let process_group = child.id() as libc::pid_t;
    let mut release_pipe = [0; 2];
    if unsafe { libc::pipe(release_pipe.as_mut_ptr()) } != 0 {
        let error = std::io::Error::last_os_error();
        terminate_child(child);
        return Err(error).context("failed to create child supervision pipe");
    }
    for fd in release_pipe {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1
        {
            let error = std::io::Error::last_os_error();
            unsafe {
                libc::close(release_pipe[0]);
                libc::close(release_pipe[1]);
            }
            terminate_child(child);
            return Err(error).context("failed to configure child supervision pipe");
        }
    }

    let watchdog_pid = unsafe { libc::fork() };
    if watchdog_pid == -1 {
        let error = std::io::Error::last_os_error();
        unsafe {
            libc::close(release_pipe[0]);
            libc::close(release_pipe[1]);
        }
        terminate_child(child);
        return Err(error).context("failed to fork child supervision watchdog");
    }
    if watchdog_pid == 0 {
        unsafe {
            libc::close(release_pipe[1]);
            let mut byte = 0_u8;
            loop {
                let read = libc::read(release_pipe[0], (&raw mut byte).cast(), 1);
                if read == 0 {
                    break;
                }
                // Keep the post-fork watchdog restricted to async-signal-safe
                // libc calls. The blocking pipe is valid for its lifetime, so
                // a negative read is transient and can be retried.
            }
            libc::close(release_pipe[0]);
            libc::kill(-process_group, libc::SIGKILL);
            libc::_exit(0);
        }
    }

    unsafe {
        libc::close(release_pipe[0]);
    }
    Ok(ChildSupervisor {
        release_fd: release_pipe[1],
        watchdog_pid,
    })
}

#[cfg(windows)]
fn terminate_child(child: &mut Child) {
    let _ = child.kill(); // guardrail: allow-ignore-result supervision setup cleanup is best-effort
    let _ = child.wait(); // guardrail: allow-ignore-result supervision setup cleanup is best-effort
}

#[cfg(unix)]
fn terminate_child(child: &mut Child) {
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.wait(); // guardrail: allow-ignore-result process-group termination is best-effort cleanup
}
