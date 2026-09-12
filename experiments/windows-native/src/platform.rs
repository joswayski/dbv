//! Win32 platform helpers: clipboard and the CSV save dialog.

use std::path::PathBuf;

use windows::core::PWSTR;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Controls::Dialogs::{
    GetSaveFileNameW, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

/// `CF_UNICODETEXT` from the Windows SDK.
const CF_UNICODETEXT: u32 = 13;

pub fn set_clipboard_text(text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(handle) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let pointer = GlobalLock(handle);
            if !pointer.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer.cast::<u16>(), wide.len());
                let _ = GlobalUnlock(handle);
                let _ = SetClipboardData(CF_UNICODETEXT, HANDLE(handle.0));
            }
        }
        let _ = CloseClipboard();
    }
}

pub fn clipboard_text() -> Option<String> {
    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }
        let result = GetClipboardData(CF_UNICODETEXT).ok().and_then(|handle| {
            let pointer = GlobalLock(HGLOBAL(handle.0));
            if pointer.is_null() {
                return None;
            }
            let mut length = 0_usize;
            let wide = pointer.cast::<u16>();
            while *wide.add(length) != 0 && length < 1_000_000 {
                length += 1;
            }
            let slice = std::slice::from_raw_parts(wide, length);
            let text = String::from_utf16_lossy(slice);
            let _ = GlobalUnlock(HGLOBAL(handle.0));
            Some(text)
        });
        let _ = CloseClipboard();
        result
    }
}

/// Shows the system "Save as" dialog and returns the chosen path.
pub fn save_csv_dialog(default_name: &str) -> Option<PathBuf> {
    let mut file_buffer = vec![0_u16; 512];
    for (index, unit) in default_name.encode_utf16().take(500).enumerate() {
        file_buffer[index] = unit;
    }
    let filter: Vec<u16> = "CSV\0*.csv\0All files\0*.*\0\0".encode_utf16().collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: crate::bridge::window().map_or(HWND(std::ptr::null_mut()), |window| window),
        lpstrFilter: windows::core::PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file_buffer.as_mut_ptr()),
        nMaxFile: file_buffer.len() as u32,
        lpstrDefExt: windows::core::w!("csv"),
        Flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    let accepted = unsafe { GetSaveFileNameW(&mut dialog) };
    if !accepted.as_bool() {
        return None;
    }
    let end = file_buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(file_buffer.len());
    let path: String = String::from_utf16_lossy(&file_buffer[..end]);
    (!path.is_empty()).then(|| PathBuf::from(path))
}
