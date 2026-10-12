//! The Windows AppUserModelID of the engine process. The engine owns the SMTC session, so the media
//! flyout names whatever this id resolves to (the installer's Start-menu shortcut). Elsewhere a no-op.

#[cfg(windows)]
pub fn set_app_user_model_id(id: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(id)) }.map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn set_app_user_model_id(_id: &str) -> Result<(), String> { Ok(()) }

#[cfg(all(test, windows))]
mod tests {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::GetCurrentProcessExplicitAppUserModelID;

    #[test]
    fn sets_the_process_id() {
        super::set_app_user_model_id("io.github.kartikeychoudhary.aurora.test").unwrap();
        let got = unsafe { GetCurrentProcessExplicitAppUserModelID() }.unwrap();
        let s = unsafe { got.to_string() }.unwrap();
        unsafe { CoTaskMemFree(Some(got.0 as *const _)) };
        assert_eq!(s, "io.github.kartikeychoudhary.aurora.test");
    }
}
