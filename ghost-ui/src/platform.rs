//! Platform-specific window configuration

use tao::window::Window;

/// Configure platform-specific window properties for transparency and click-through behavior.
pub fn configure_window(window: &Window, click_through: bool) {
    #[cfg(target_os = "macos")]
    configure_macos(window, click_through);

    #[cfg(target_os = "windows")]
    configure_windows(window, click_through);

    #[cfg(target_os = "linux")]
    configure_linux(window, click_through);
}

#[cfg(target_os = "macos")]
#[allow(unused_imports)]
fn configure_macos(window: &Window, click_through: bool) {
    use tao::platform::macos::WindowExtMacOS;
    if click_through {
        let _ = window.set_ignore_cursor_events(true);
    }
}

#[cfg(target_os = "windows")]
fn configure_windows(window: &Window, _click_through: bool) {
    use tao::platform::windows::WindowExtWindows;
    use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, MARGINS};
    use windows::Win32::Foundation::HWND;

    unsafe {
        let hwnd = window.hwnd();
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(HWND(hwnd as _), &margins);
    }
}

#[cfg(target_os = "linux")]
fn configure_linux(_window: &Window, _click_through: bool) {
    // Linux transparency is handled via compositor settings
    // Click-through requires X11-specific extensions not commonly available
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn configure_window(_window: &Window, _click_through: bool) {
    // Fallback for other platforms
}
