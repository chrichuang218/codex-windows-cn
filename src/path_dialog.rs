//! Native folder-picker via IFileOpenDialog with FOS_PICKFOLDERS.
//! Returns `Ok(None)` if the user cancelled.

use anyhow::{Context, Result};
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

/// User-cancelled = `0x800704C7` (ERROR_CANCELLED wrapped as HRESULT).
const ERROR_CANCELLED_HR: u32 = 0x800704C7;

pub fn pick_folder() -> Result<Option<PathBuf>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .context("CoCreateInstance(FileOpenDialog)")?;
        let opts = dialog.GetOptions().context("GetOptions")?;
        dialog
            .SetOptions(opts | FOS_PICKFOLDERS)
            .context("SetOptions")?;

        match dialog.Show(HWND::default()) {
            Ok(()) => {}
            Err(e) if e.code().0 as u32 == ERROR_CANCELLED_HR => {
                CoUninitialize();
                return Ok(None);
            }
            Err(e) => {
                CoUninitialize();
                return Err(e).context("IFileOpenDialog::Show");
            }
        }

        let item: IShellItem = dialog.GetResult().context("GetResult")?;
        let pw = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .context("GetDisplayName")?;
        let mut len = 0usize;
        while *pw.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(pw.0, len);
        let path = String::from_utf16_lossy(slice);
        CoTaskMemFree(Some(pw.0 as *mut _));
        CoUninitialize();
        Ok(Some(PathBuf::from(path)))
    }
}
