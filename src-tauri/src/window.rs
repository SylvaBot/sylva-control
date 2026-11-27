//! Moving the window from Sylva Control's own title bar.
//!
//! On Windows, a window that has been snapped (dragged to a screen edge, Win+arrow, a snap
//! layout, or resized until its top or bottom edge touched the screen edge) remembers the size
//! it had before the snap. Starting a drag then puts that old size back, so the window seems to
//! jump to a default size. Before each drag Sylva Control tells Windows that the current size is the
//! window's normal size, so moving it never changes its size. A maximized window is left alone:
//! dragging it still restores it, like any other app.

use tauri::WebviewWindow;

/// Starts moving the window with the mouse. Called on mouse down in the title bar.
#[tauri::command]
pub fn start_window_drag(window: WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    keep_current_size(&window);
    window.start_dragging().map_err(|e| e.to_string())
}

#[cfg(windows)]
fn keep_current_size(window: &WebviewWindow) {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, GetWindowRect, IsIconic, IsZoomed, SetWindowPlacement, SetWindowPos,
        SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNORMAL, WINDOWPLACEMENT, WINDOWPLACEMENT_FLAGS,
    };

    let Ok(handle) = window.hwnd() else { return };
    let hwnd = HWND(handle.0 as _);

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
