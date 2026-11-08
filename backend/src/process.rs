//! End the service when its owning desktop process disappears, including crashes.
use anyhow::Result;

#[cfg(windows)]
pub fn watch_parent(pid: u32) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    // The handle observes only process lifetime; it grants no mutation rights.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        anyhow::bail!("桌面宿主进程已经退出");
    }
    let handle = handle as usize;
    std::thread::spawn(move || {
        unsafe {
            WaitForSingleObject(handle as _, INFINITE);
            CloseHandle(handle as _);
        }
        std::process::exit(0);
    });
    Ok(())
}

#[cfg(not(windows))]
pub fn watch_parent(_pid: u32) -> Result<()> {
    Ok(())
}
