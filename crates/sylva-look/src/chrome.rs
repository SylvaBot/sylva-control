//! The frameless window: Sylva's title bar (leaf, name, window buttons, drag to move,
//! double click to maximize) and resizing from the edges.

use crate::icons::Icons;
use crate::theme::*;
use crate::widgets::hand;
use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, Id, Rect, Response, Sense, ViewportCommand};

/// One of the buttons at the right of the title bar, 36 by 30.
pub fn win_button(ui: &mut egui::Ui, icons: &Icons, rect: Rect, icon: &str, danger: bool, pressed: bool, tip: &str) -> Response {
    let r = ui.interact(rect, Id::unique(("win-button", icon)), Sense::click()).on_hover_text(tip);
    let bg = if r.hovered() && danger {
        DANGER_SOLID
    } else if r.hovered() || pressed {
        BTN
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, R_CONTROL, bg);
    let tint = if r.hovered() || pressed { if danger { Color32::WHITE } else { TEXT } } else { MUTED };
    icons.paint(ui.painter(), icon, rect.center(), 13.0, tint);
    hand(ui, &r);
    r
}

/// Draws the title bar and handles moving the window. `before_drag` runs just before a drag
/// starts (Sylva Control uses it on Windows so a snapped window keeps its size). Returns the x
/// where more buttons can be placed, right to left, before the window buttons.
pub fn titlebar(ui: &mut egui::Ui, icons: &Icons, rect: Rect, title: &str, show_title: bool, before_drag: impl FnOnce()) -> f32 {
    let ctx = ui.ctx().clone();
    let drag = ui.interact(rect, Id::unique("titlebar-drag"), Sense::click_and_drag());
    if drag.double_clicked() {
        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!max));
    } else if drag.drag_started_by(egui::PointerButton::Primary) {
        before_drag();
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let p = ui.painter().clone();
    let cy = rect.center().y;
    let x = rect.left() + if show_title { 18.0 } else { 14.0 };
    p.image(icons.get("leaf").id(), Rect::from_min_size(pos2(x, cy - 11.0), vec2(16.0, 22.0)), crate::widgets::uv(), Color32::WHITE);
    if show_title {
        let g = p.layout_no_wrap(title.to_string(), semibold(14.0), TEXT);
        p.galley(pos2(x + 25.0, cy - g.size().y / 2.0), g, TEXT);
    }
    let mut r = rect.right() - 8.0;
    let mut next = |w: f32| {
        r -= w;
        let b = Rect::from_min_size(pos2(r, cy - 15.0), vec2(w, 30.0));
        r -= 2.0;
        b
    };
    let (close, max, min) = (next(36.0), next(36.0), next(36.0));
    if win_button(ui, icons, close, "x", true, false, "Close").clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }
    if win_button(ui, icons, max, "square", false, false, "Maximize").clicked() {
        let m = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!m));
    }
    if win_button(ui, icons, min, "minus", false, false, "Minimize").clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
    }
    r - 6.0
}

/// Resizing from the window edges (the window has no system frame).
pub fn edges(ctx: &egui::Context) {
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let r = ctx.content_rect();
    let m = 5.0;
    let (l, rt, t, b) = (p.x < r.left() + m, p.x > r.right() - m, p.y < r.top() + m, p.y > r.bottom() - m);
    use egui::ResizeDirection as D;
    let dir = match (l, rt, t, b) {
        (true, _, true, _) => Some((D::NorthWest, CursorIcon::ResizeNorthWest)),
        (_, true, true, _) => Some((D::NorthEast, CursorIcon::ResizeNorthEast)),
        (true, _, _, true) => Some((D::SouthWest, CursorIcon::ResizeSouthWest)),
        (_, true, _, true) => Some((D::SouthEast, CursorIcon::ResizeSouthEast)),
        (true, ..) => Some((D::West, CursorIcon::ResizeWest)),
        (_, true, ..) => Some((D::East, CursorIcon::ResizeEast)),
        (_, _, true, _) => Some((D::North, CursorIcon::ResizeNorth)),
        (_, _, _, true) => Some((D::South, CursorIcon::ResizeSouth)),
        _ => None,
    };
    if let Some((d, cursor)) = dir {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(d));
        }
    }
}
