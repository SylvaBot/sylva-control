//! Phosphor icons (MIT, see assets/icons/LICENSE-Phosphor.txt), drawn white and tinted when
//! painted, plus Sylva's leaf.

use crate::images::to_color_image;
use eframe::egui::{self, TextureHandle, TextureOptions};
use std::collections::HashMap;

pub struct Icons {
    map: HashMap<&'static str, TextureHandle>,
}

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        let mut map = HashMap::new();
        macro_rules! icon {
            ($($name:literal),* $(,)?) => {$(
                let img = image::load_from_memory(include_bytes!(concat!("../assets/icons/", $name, ".png"))).expect("icon").to_rgba8();
                map.insert($name, ctx.load_texture($name, to_color_image(&img), TextureOptions::LINEAR));
            )*};
        }
        icon!(
            "activity", "arrow-up", "check", "chevron", "clock", "download", "edit", "external", "eye", "gem", "gift",
            "heart", "home", "image", "info", "layers", "leaf", "logout", "minus", "monitor", "music", "pause", "play",
            "raid", "refresh", "replay", "search", "settings", "skip", "sparkles", "square", "star", "timer", "tip",
            "trash", "twitch", "user", "vol", "volx", "warning", "x", "zap",
        );
        Self { map }
    }

    /// The icon by name. Unknown names fall back to "sparkles" rather than panicking.
    pub fn get(&self, name: &str) -> &TextureHandle {
        self.map.get(name).unwrap_or(&self.map["sparkles"])
    }

    /// Paints an icon tinted, centered in `rect` at `size` points.
    pub fn paint(&self, p: &egui::Painter, name: &str, center: egui::Pos2, size: f32, tint: egui::Color32) {
        let r = egui::Rect::from_center_size(center, egui::vec2(size, size));
        let (w, h) = if name == "leaf" { (size * 0.72, size) } else { (size, size) };
        let r = egui::Rect::from_center_size(r.center(), egui::vec2(w, h));
        p.image(self.get(name).id(), r, crate::widgets::uv(), tint);
    }
}
