//! Observe the spawned image without substituting our own executable lookup.

use serde_json::{Value, json};
use std::process::Child;

pub(crate) fn observe(child: &Child) -> Value {
    match image_path(child) {
        Ok(path) => json!({"path": path, "source": "windows_process_handle", "error": null}),
        Err(error) => json!({"path": null, "source": "unobserved", "error": error}),
    }
}

#[cfg(windows)]
fn image_path(child: &Child) -> Result<String, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;
    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    // The Child owns a live process handle, including after fast process exit.
    let success = unsafe {
        QueryFullProcessImageNameW(child.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut size)
    };
    if success == 0 {
        return Err(format!(
            "process image query failed: {}; use an absolute executable path to make native selection explicit",
            std::io::Error::last_os_error()
        ));
    }
    String::from_utf16(&buffer[..size as usize])
        .map_err(|error| format!("process image path is not valid Unicode: {error}"))
}

#[cfg(not(windows))]
fn image_path(_child: &Child) -> Result<String, String> {
    Err("image observation is unavailable on this platform; native PATH or interpreter resolution is not observed; use an absolute executable path to make selection explicit".into())
}
