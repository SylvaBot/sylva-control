//! Sylva's look, by hand: the same colors, type, radii and spacing as Sylva's website.
//! Flat near black, Geist, solid filled surfaces, and sage only on the main action
//! and whatever is on or selected. No glass, blur, gradients, colored left borders or dot badges.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId};
use std::sync::Arc;

pub const FRAME: Color32 = Color32::from_rgb(0x05, 0x05, 0x05);
pub const BG: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0a);
pub const SURFACE: Color32 = Color32::from_rgb(0x14, 0x14, 0x13);
pub const SURFACE_2: Color32 = Color32::from_rgb(0x22, 0x22, 0x21);
pub const SURFACE_3: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2a);
pub const POP: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1b);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x19);
pub const BTN: Color32 = Color32::from_rgb(0x22, 0x22, 0x21);
pub const BTN_HOVER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2a);
pub const BTN_IN: Color32 = Color32::from_rgb(0x26, 0x26, 0x25);
pub const BTN_IN_HOVER: Color32 = Color32::from_rgb(0x30, 0x30, 0x2f);
pub const TEXT: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf0);
pub const TEXT_2: Color32 = Color32::from_rgb(0xc9, 0xc9, 0xc4);
pub const MUTED: Color32 = Color32::from_rgb(0xa3, 0xa3, 0x9e);
pub const FAINT: Color32 = Color32::from_rgb(0x8a, 0x8a, 0x84);
pub const SAGE: Color32 = Color32::from_rgb(0xd6, 0xef, 0xc2);
pub const SAGE_HOVER: Color32 = Color32::from_rgb(0xe3, 0xf6, 0xd4);
pub const ON_SAGE: Color32 = Color32::from_rgb(0x11, 0x16, 0x0e);
pub const TAG_BG: Color32 = Color32::from_rgb(0x1b, 0x22, 0x18);
pub const HL_HOVER: Color32 = Color32::from_rgb(0x21, 0x2a, 0x1d);
pub const DANGER: Color32 = Color32::from_rgb(0xf5, 0x94, 0x96);
pub const DRAWER: Color32 = Color32::from_rgb(0x11, 0x11, 0x10);
pub const ROW_ON: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1b);
pub const FIELD: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x19);
pub const FIELD_IN: Color32 = Color32::from_rgb(0x26, 0x26, 0x25);
pub const TILE: Color32 = Color32::from_rgb(0x26, 0x26, 0x25);
pub const TILE_INK: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe2);
pub const SW_OFF: Color32 = Color32::from_rgb(0x33, 0x33, 0x32);
pub const KNOB_OFF: Color32 = Color32::from_rgb(0xe9, 0xe9, 0xe4);
pub const WARN: Color32 = Color32::from_rgb(0xe8, 0xc3, 0x6a);
pub const SCRIM: Color32 = Color32::from_black_alpha(184);
pub const DANGER_SOLID: Color32 = Color32::from_rgb(0xc4, 0x36, 0x3b);

/// 8px small controls and chips, 12px fields, rows and cards, 20px dialogs and the page panel.
pub const R_CONTROL: CornerRadius = CornerRadius::same(8);
pub const R_ITEM: CornerRadius = CornerRadius::same(12);
pub const R_PANEL: u8 = 20;

pub const TITLEBAR: f32 = 44.0;

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn parse_hex(hex: &str) -> Color32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16)
        .map(|n| Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
        .unwrap_or(TEXT)
}

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "geist", include_bytes!("../assets/fonts/Geist-Regular.ttf"));
    add(&mut fonts, "geist-medium", include_bytes!("../assets/fonts/Geist-Medium.ttf"));
    add(&mut fonts, "geist-semibold", include_bytes!("../assets/fonts/Geist-SemiBold.ttf"));
    add(&mut fonts, "geist-bold", include_bytes!("../assets/fonts/Geist-Bold.ttf"));
    add(&mut fonts, "geist-mono", include_bytes!("../assets/fonts/GeistMono-Regular.ttf"));
    // egui's own fonts stay behind Geist for emoji and symbols Geist lacks.
    let fallbacks = fonts.families[&FontFamily::Proportional].clone();
    let family = |first: &str| {
        let mut v = vec![first.to_string()];
        v.extend(fallbacks.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, family("geist"));
    fonts.families.insert(FontFamily::Name("medium".into()), family("geist-medium"));
    fonts.families.insert(FontFamily::Name("semibold".into()), family("geist-semibold"));
    fonts.families.insert(FontFamily::Name("bold".into()), family("geist-bold"));
    let mut mono_family = vec!["geist-mono".to_string()];
    mono_family.extend(fonts.families[&FontFamily::Monospace].iter().cloned());
    fonts.families.insert(FontFamily::Monospace, mono_family);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|s| {
        let v = &mut s.visuals;
        v.panel_fill = FRAME;
        v.window_fill = POP;
        v.extreme_bg_color = SURFACE;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = SAGE;
        v.selection.bg_fill = Color32::from_rgba_unmultiplied(0xd6, 0xef, 0xc2, 90);
        v.selection.stroke.color = SAGE;
        v.text_cursor.stroke.color = SAGE;
        v.window_stroke.width = 0.0;
        v.window_corner_radius = CornerRadius::same(R_PANEL);
        v.menu_corner_radius = R_ITEM;
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive, &mut v.widgets.open] {
            w.corner_radius = R_CONTROL;
            w.bg_stroke.width = 0.0;
            w.fg_stroke.color = TEXT;
        }
        v.widgets.inactive.weak_bg_fill = BTN;
        v.widgets.inactive.bg_fill = BTN;
        v.widgets.hovered.weak_bg_fill = BTN_HOVER;
        v.widgets.hovered.bg_fill = BTN_HOVER;
        v.widgets.active.weak_bg_fill = SURFACE_3;
        v.widgets.active.bg_fill = SURFACE_3;
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 8.0);
        s.spacing.scroll.bar_width = 4.0;
        s.spacing.scroll.floating = true;
        s.interaction.selectable_labels = false;
        s.text_styles.insert(egui::TextStyle::Body, regular(14.0));
        s.text_styles.insert(egui::TextStyle::Button, medium(13.5));
        s.text_styles.insert(egui::TextStyle::Small, regular(12.0));
        s.text_styles.insert(egui::TextStyle::Monospace, mono(13.0));
        s.text_styles.insert(egui::TextStyle::Heading, medium(24.0));
    });
}
