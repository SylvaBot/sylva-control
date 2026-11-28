//! Controls in Sylva's look. Each one allocates its own space in a `Ui` (or paints into a
//! given rect) and returns egui's `Response`, so callers read clicks the usual way.

use crate::icons::Icons;
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, CursorIcon, Galley, Id, Pos2, Rect, Response, Sense, Stroke, UiBuilder};
use eframe::egui::text::{LayoutJob, TextFormat};
use std::sync::Arc;

pub fn uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

pub fn shadow() -> egui::Shadow {
    egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(110) }
}

pub fn popup_frame(radius: u8, pad: i8) -> egui::Frame {
    egui::Frame::new().fill(POP).corner_radius(CornerRadius::same(radius)).inner_margin(pad).shadow(shadow())
}

/// The pointing hand over anything clickable.
pub fn hand(ui: &egui::Ui, r: &Response) {
    if r.hovered() && r.sense.senses_click() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
}

pub fn fmt(font: egui::FontId, color: Color32) -> TextFormat {
    TextFormat { font_id: font, color, ..Default::default() }
}

/// One line of text cut with an ellipsis at `max_w`.
pub fn line(p: &egui::Painter, text: impl Into<String>, font: egui::FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.into(), fmt(font, color));
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    p.layout_job(job)
}

/// Wrapped text, at most `rows` lines (0 for no limit).
pub fn para(p: &egui::Painter, text: impl Into<String>, font: egui::FontId, color: Color32, max_w: f32, rows: usize) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.into(), fmt(font, color));
    job.wrap.max_width = max_w.max(1.0);
    if rows > 0 {
        job.wrap.max_rows = rows;
        job.wrap.overflow_character = Some('…');
    }
    p.layout_job(job)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Default,
    Primary,
    Ghost,
    Danger,
    /// A chip that is on.
    Pressed,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Size {
    /// 30 tall.
    Sm,
    /// 36 tall.
    Md,
    /// 40 tall with 12px corners, as in dialogs.
    Dialog,
    /// 44 tall with 12px corners.
    Lg,
}

pub struct Button<'a> {
    text: &'a str,
    icon: Option<&'a str>,
    icon_after: bool,
    kind: Kind,
    size: Size,
    in_card: bool,
    enabled: bool,
    width: Option<f32>,
    spin: bool,
    tip: Option<&'a str>,
}

impl<'a> Button<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, icon: None, icon_after: false, kind: Kind::Default, size: Size::Md, in_card: false, enabled: true, width: None, spin: false, tip: None }
    }
    pub fn primary(mut self) -> Self {
        self.kind = Kind::Primary;
        self
    }
    pub fn ghost(mut self) -> Self {
        self.kind = Kind::Ghost;
        self
    }
    pub fn danger(mut self) -> Self {
        self.kind = Kind::Danger;
        self
    }
    pub fn pressed(mut self, on: bool) -> Self {
        if on {
            self.kind = Kind::Pressed;
        }
        self
    }
    pub fn size(mut self, s: Size) -> Self {
        self.size = s;
        self
    }
    pub fn sm(self) -> Self {
        self.size(Size::Sm)
    }
    pub fn icon(mut self, name: &'a str) -> Self {
        self.icon = Some(name);
        self
    }
    /// The icon goes after the words, like a chevron.
    pub fn icon_after(mut self, name: &'a str) -> Self {
        self.icon = Some(name);
        self.icon_after = true;
        self
    }
    /// Inside a card the default button is a step lighter.
    pub fn in_card(mut self, v: bool) -> Self {
        self.in_card = v;
        self
    }
    pub fn enabled(mut self, v: bool) -> Self {
        self.enabled = v;
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }
    /// Turns the icon, while something loads.
    pub fn spin(mut self, v: bool) -> Self {
        self.spin = v;
        self
    }
    pub fn tip(mut self, t: &'a str) -> Self {
        self.tip = Some(t);
        self
    }

    fn metrics(&self) -> (f32, f32, f32, f32, CornerRadius) {
        // height, side padding, font size, icon size, corners
        match self.size {
            Size::Sm => (30.0, 10.0, 12.5, 14.0, R_CONTROL),
            Size::Md => (36.0, 13.0, 13.5, 15.0, R_CONTROL),
            Size::Dialog => (40.0, 14.0, 13.5, 15.0, R_ITEM),
            Size::Lg => (44.0, 18.0, 14.0, 15.0, R_ITEM),
        }
    }

    pub fn measure(&self, ui: &egui::Ui) -> egui::Vec2 {
        let (h, pad, font, icon, _) = self.metrics();
        let g = ui.painter().layout_no_wrap(self.text.to_string(), medium(font), TEXT);
        let mut w = g.size().x + 2.0 * pad;
        if self.icon.is_some() {
            w += icon + if self.text.is_empty() { 0.0 } else { 7.0 };
        }
        vec2(self.width.unwrap_or(w), h)
    }

    pub fn show(self, ui: &mut egui::Ui, icons: &Icons) -> Response {
        let size = self.measure(ui);
        let (rect, resp) = ui.allocate_exact_size(size, if self.enabled { Sense::click() } else { Sense::hover() });
        self.paint(ui, icons, rect, resp)
    }

    /// Draws the button in a rect the caller chose.
    pub fn show_at(self, ui: &mut egui::Ui, icons: &Icons, rect: Rect, id: Id) -> Response {
        let resp = ui.interact(rect, id, if self.enabled { Sense::click() } else { Sense::hover() });
        self.paint(ui, icons, rect, resp)
    }

    fn paint(self, ui: &mut egui::Ui, icons: &Icons, rect: Rect, mut resp: Response) -> Response {
        let (_, pad, font, icon_s, radius) = self.metrics();
        let hover = self.enabled && resp.hovered();
        let (fill, ink) = match self.kind {
            Kind::Primary => (if hover { SAGE_HOVER } else { SAGE }, ON_SAGE),
            Kind::Pressed => (SAGE, ON_SAGE),
            Kind::Ghost => (if hover { BTN } else { Color32::TRANSPARENT }, if hover { TEXT } else { MUTED }),
            Kind::Default | Kind::Danger => {
                let f = match (self.in_card, hover) {
                    (true, true) => BTN_IN_HOVER,
                    (true, false) => BTN_IN,
                    (false, true) => BTN_HOVER,
                    (false, false) => BTN,
                };
                (f, if self.kind == Kind::Danger { DANGER } else { TEXT })
            }
        };
        let dim = if self.enabled { 1.0 } else { 0.45 };
        let p = ui.painter();
        p.rect_filled(rect, radius, fill.gamma_multiply(dim));
        let g = p.layout_no_wrap(self.text.to_string(), medium(font), ink.gamma_multiply(dim));
        let gap = if self.text.is_empty() { 0.0 } else { 7.0 };
        let total = g.size().x + self.icon.map_or(0.0, |_| icon_s + gap);
        let mut x = (rect.center().x - total / 2.0).max(rect.left() + pad.min((rect.width() - total) / 2.0).max(0.0));
        let cy = rect.center().y;
        let draw_icon = |x: f32, name: &str| {
            let r = Rect::from_center_size(pos2(x + icon_s / 2.0, cy), vec2(icon_s, icon_s));
            if self.spin {
                let t = ui.input(|i| i.time) as f32;
                egui::Image::new(egui::load::SizedTexture::new(icons.get(name).id(), r.size()))
                    .tint(ink.gamma_multiply(dim))
                    .rotate(t * std::f32::consts::TAU / 0.8, vec2(0.5, 0.5))
                    .paint_at(ui, r);
                ui.ctx().request_repaint();
            } else {
                ui.painter().image(icons.get(name).id(), r, uv(), ink.gamma_multiply(dim));
            }
        };
        if let (Some(name), false) = (self.icon, self.icon_after) {
            draw_icon(x, name);
            x += icon_s + gap;
        }
        ui.painter().galley(pos2(x, cy - g.size().y / 2.0), g.clone(), TEXT);
        x += g.size().x + gap;
        if let (Some(name), true) = (self.icon, self.icon_after) {
            draw_icon(x, name);
        }
        if let Some(t) = self.tip {
            resp = resp.on_hover_text(t);
        }
        hand(ui, &resp);
        resp
    }
}

/// A 32 by 32 icon button with no fill until hovered.
pub fn icon_button(ui: &mut egui::Ui, icons: &Icons, rect: Rect, id: Id, name: &str, tip: &str, enabled: bool) -> Response {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
    let hover = enabled && resp.hovered();
    if hover {
        ui.painter().rect_filled(rect, R_CONTROL, BTN);
    }
    let tint = if !enabled { MUTED.gamma_multiply(0.45) } else if hover { TEXT } else { MUTED };
    icons.paint(ui.painter(), name, rect.center(), 16.0, tint);
    hand(ui, &resp);
    resp
}

/// A 40 by 22 switch. Returns the response; the caller flips its value on click.
pub fn switch(ui: &mut egui::Ui, rect: Rect, id: Id, on: bool, tip: &str) -> Response {
    let resp = ui.interact(rect, id, Sense::click()).on_hover_text(tip);
    let t = ui.ctx().animate_bool_with_time(id.with("anim"), on, 0.15);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(11), lerp(SW_OFF, SAGE, t));
    let knob = pos2(rect.left() + 11.0 + 18.0 * t, rect.center().y);
    p.circle_filled(knob, 8.0, lerp(KNOB_OFF, ON_SAGE, t));
    hand(ui, &resp);
    resp
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
}

/// The rounded square that holds an icon at the start of a row.
pub fn tile(p: &egui::Painter, icons: &Icons, rect: Rect, name: &str, fill: Color32, ink: Color32) {
    let radius = if rect.width() >= 40.0 { R_ITEM } else { R_CONTROL };
    p.rect_filled(rect, radius, fill);
    let s = if rect.width() >= 40.0 { 19.0 } else if rect.width() < 30.0 { 13.0 } else { 16.0 };
    icons.paint(p, name, rect.center(), if name == "leaf" { s + 1.0 } else { s }, ink);
}

/// A grey placeholder the size of what is coming, breathing gently.
pub fn skel(ui: &egui::Ui, rect: Rect) {
    let t = ui.input(|i| i.time);
    let a = ((t * std::f64::consts::TAU / 1.8).sin() * 0.5 + 0.5) as f32;
    ui.painter().rect_filled(rect, R_CONTROL, lerp(SURFACE_2, SURFACE_3, a));
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
}

pub struct FieldOpts<'a> {
    pub hint: &'a str,
    pub in_card: bool,
    pub mono: bool,
    pub multiline: bool,
}

impl FieldOpts<'_> {
    /// Leaves room on the left for an icon the caller paints.
    pub fn with_icon(self) -> IconField<Self> {
        IconField(self)
    }
}

/// A field with room for a leading icon.
pub struct IconField<T>(pub T);

/// A text field: filled, 12px corners, a sage ring while typing.
pub fn field<'a>(ui: &mut egui::Ui, rect: Rect, id: Id, text: &mut String, o: impl Into<IconOr<'a>>) -> Response {
    let IconOr(o, lead) = o.into();
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    ui.painter().rect_filled(rect, R_ITEM, if o.in_card { FIELD_IN } else { FIELD });
    if focused {
        ui.painter().rect_stroke(rect.shrink(1.0), R_ITEM, Stroke::new(2.0, SAGE), egui::StrokeKind::Inside);
    }
    let font = if o.mono { mono(13.0) } else { regular(13.5) };
    let mut inner = if o.multiline { rect.shrink2(vec2(12.0, 8.0)) } else { rect.shrink2(vec2(12.0, 0.0)) };
    inner.min.x += lead;
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        let edit = if o.multiline { egui::TextEdit::multiline(text).desired_rows(1) } else { egui::TextEdit::singleline(text) };
        ui.add_sized(
            inner.size(),
            edit.id(id)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(egui::RichText::new(o.hint).color(FAINT).font(font.clone()))
                .font(font)
                .text_color(TEXT)
                .desired_width(inner.width())
                .align(if o.multiline { egui::Align2::LEFT_TOP } else { egui::Align2::LEFT_CENTER }),
        )
    })
    .inner
}

pub struct IconOr<'a>(FieldOpts<'a>, f32);

impl<'a> From<FieldOpts<'a>> for IconOr<'a> {
    fn from(o: FieldOpts<'a>) -> Self {
        IconOr(o, 0.0)
    }
}

impl<'a> From<IconField<FieldOpts<'a>>> for IconOr<'a> {
    fn from(o: IconField<FieldOpts<'a>>) -> Self {
        IconOr(o.0, 24.0)
    }
}

pub struct Toast {
    text: String,
    bad: bool,
    until: f64,
}

/// Short notes at the bottom of the window.
#[derive(Default)]
pub struct Toasts {
    list: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, ctx: &egui::Context, text: impl Into<String>, bad: bool, secs: f64) {
        let now = ctx.input(|i| i.time);
        self.list.push(Toast { text: text.into(), bad, until: now + secs });
        if self.list.len() > 4 {
            self.list.remove(0);
        }
        ctx.request_repaint();
    }

    pub fn show(&mut self, ctx: &egui::Context, bottom: f32) {
        let now = ctx.input(|i| i.time);
        self.list.retain(|t| t.until > now);
        if self.list.is_empty() {
            return;
        }
        let screen = ctx.content_rect();
        let mut y = bottom;
        let mut soonest = f64::MAX;
        for (i, t) in self.list.iter().enumerate().rev() {
            let alpha = ((t.until - now) / 0.15).clamp(0.0, 1.0) as f32;
            soonest = soonest.min(t.until - now - 0.15);
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique(("toast", i))));
            let color = if t.bad { DANGER } else { TEXT };
            let g = p.layout(t.text.clone(), regular(13.5), color, (screen.width() - 60.0).min(460.0));
            let size = g.size() + vec2(28.0, 20.0);
            let rect = Rect::from_min_size(pos2(screen.center().x - size.x / 2.0, y - size.y), size);
            y -= size.y + 8.0;
            p.add(shadow().as_shape(rect, R_ITEM));
            p.rect_filled(rect, R_ITEM, POP.gamma_multiply(alpha));
            p.galley_with_override_text_color(rect.min + vec2(14.0, 10.0), g, color.gamma_multiply(alpha));
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(soonest.max(0.016)));
    }
}

/// A round avatar: the picture when it loads, the first letter on sage until then.
pub fn avatar(p: &egui::Painter, images: &crate::Images, center: Pos2, size: f32, name: &str, url: Option<&str>) {
    let r = Rect::from_center_size(center, vec2(size, size));
    if let Some(a) = url.filter(|u| u.starts_with("https://")).and_then(|u| images.get(u)) {
        let (tex, _) = a.frame(0);
        // Round by drawing the picture as a textured circle.
        let mut mesh = egui::Mesh::with_texture(tex.id());
        let n = 48;
        let c = center;
        mesh.vertices.push(egui::epaint::Vertex { pos: c, uv: pos2(0.5, 0.5), color: Color32::WHITE });
        for i in 0..=n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let (s, co) = a.sin_cos();
            mesh.vertices.push(egui::epaint::Vertex { pos: c + vec2(co, s) * size / 2.0, uv: pos2(0.5 + co / 2.0, 0.5 + s / 2.0), color: Color32::WHITE });
        }
        for i in 1..=n as u32 {
            mesh.indices.extend([0, i, i + 1]);
        }
        p.add(mesh);
        return;
    }
    p.circle_filled(r.center(), size / 2.0, SAGE);
    let letter = name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    p.text(r.center(), egui::Align2::CENTER_CENTER, letter, semibold(size * 0.41), ON_SAGE);
}

/// Fits a picture of `size` inside `slot`, keeping its shape.
pub fn fit_rect(slot: Rect, size: egui::Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return slot;
    }
    let s = (slot.width() / size.x).min(slot.height() / size.y);
    Rect::from_center_size(slot.center(), size * s)
}

/// Fills `slot` with a picture, cropping what does not fit (like CSS object-fit: cover).
pub fn cover_uv(slot: Rect, size: egui::Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return uv();
    }
    let (sa, ia) = (slot.width() / slot.height(), size.x / size.y);
    if ia > sa {
        let w = sa / ia;
        Rect::from_min_max(pos2(0.5 - w / 2.0, 0.0), pos2(0.5 + w / 2.0, 1.0))
    } else {
        let h = ia / sa;
        Rect::from_min_max(pos2(0.0, 0.5 - h / 2.0), pos2(1.0, 0.5 + h / 2.0))
    }
}
