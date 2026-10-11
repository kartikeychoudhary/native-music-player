//! A hidden window owned by the engine process, which SMTC needs (`GetForWindow`), plus the message
//! pump that delivers its media-button events.
use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, RegisterClassExW, TranslateMessage, MSG,
    PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW,
};

pub struct HiddenWindow { hwnd: HWND }

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

impl HiddenWindow {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = w!("AuroraMediaWindow");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                hInstance: instance.into(),
                lpszClassName: class,
                lpfnWndProc: Some(wnd_proc),
                ..Default::default()
            };
            if RegisterClassExW(&wc) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE::default(), class, w!("Aurora"), WINDOW_STYLE::default(), 0, 0, 0, 0, None, None, Some(instance.into()), None)
                .map_err(|e| e.to_string())?;
            Ok(HiddenWindow { hwnd })
        }
    }

    pub fn hwnd(&self) -> *mut c_void { self.hwnd.0 }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) { unsafe { let _ = DestroyWindow(self.hwnd); } }
}

/// Drains this thread's message queue (souvlaki's SMTC events arrive as window messages).
pub fn pump() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
