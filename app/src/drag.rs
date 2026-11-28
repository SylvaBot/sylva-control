//! Moving the window from Sylva Control's own title bar.
//!
//! On Windows, a window that has been snapped (dragged to a screen edge, Win+arrow, a snap
//! layout, or resized until its top or bottom edge touched the screen edge) remembers the size
//! it had before the snap. Starting a drag then puts that old size back, so the window seems to
//! jump to a default size. Before each drag Sylva Control tells Windows that the current size is
//! the window's normal size, so moving it never changes its size. A maximized window is left
//! alone: dragging it still restores it, like any other app.

/// The window's handle on Windows, read once at start.
#[cfg(windows)]
pub fn hwnd(cc: &eframe::CreationContext<'_>) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
pub fn hwnd(_cc: &eframe::CreationContext<'_>) -> Option<isize> {
    None
}

#[cfg(not(windows))]
pub fn keep_current_size(_hwnd: Option<isize>) {}

#[cfg(windows)]
pub fn keep_current_size(hwnd: Option<isize>) {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, GetWindowRect, IsIconic, IsZoomed, SetWindowPlacement, SetWindowPos,
        SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNORMAL, WINDOWPLACEMENT, WINDOWPLACEMENT_FLAGS,
    };

    let Some(h) = hwnd else { return };
    let hwnd = HWND(h as _);

    unsafe {
        if IsZoomed(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let mut placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        if GetWindowPlacement(hwnd, &mut placement).is_err() {
            return;
        }
        let normal = placement.rcNormalPosition;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if normal.right - normal.left == width && normal.bottom - normal.top == height {
            return; // Not snapped: a drag already keeps this size.
        }

        // The normal position is in workspace coordinates, which start at the top left of the
        // primary monitor's work area (the part the taskbar does not cover).
        let primary = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(primary, &mut info).as_bool() {
            return;
        }
        let dx = info.rcWork.left - info.rcMonitor.left;
        let dy = info.rcWork.top - info.rcMonitor.top;
        placement.rcNormalPosition = RECT {
            left: rect.left - dx,
            top: rect.top - dy,
            right: rect.right - dx,
            bottom: rect.bottom - dy,
        };
        placement.showCmd = SW_SHOWNORMAL.0 as u32;
        placement.flags = WINDOWPLACEMENT_FLAGS(0);
        let _ = SetWindowPlacement(hwnd, &placement);
        // Make sure the window sits exactly where it was. This does nothing when it already does.
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            rect.top,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}
