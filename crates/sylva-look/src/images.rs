//! Emote and badge images: fetched once, decoded off the UI thread (GIF and WebP animations
//! included), and kept as GPU textures. Animated ones only cost a redraw while on screen.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use image::{AnimationDecoder, ImageFormat};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

const MAX_FRAMES: usize = 400;

pub struct Anim {
    frames: Vec<(TextureHandle, u32)>,
    total_ms: u64,
    /// Size in image pixels.
    pub size: egui::Vec2,
}

impl Anim {
    /// The frame to show now, and how long until the next one (None for still images).
    pub fn frame(&self, now_ms: u64) -> (&TextureHandle, Option<u64>) {
        if self.frames.len() == 1 || self.total_ms == 0 {
            return (&self.frames[0].0, None);
        }
        let mut t = now_ms % self.total_ms;
        for (tex, ms) in &self.frames {
            let ms = *ms as u64;
            if t < ms {
                return (tex, Some(ms - t));
            }
            t -= ms;
        }
        (&self.frames[0].0, Some(1))
    }
}

enum Slot {
    Loading,
    Failed,
    Ready(Arc<Anim>),
}

#[derive(Clone)]
pub struct Images {
    ctx: egui::Context,
    http: reqwest::Client,
    rt: tokio::runtime::Handle,
    map: Arc<Mutex<HashMap<String, Slot>>>,
}

impl Images {
    pub fn new(ctx: egui::Context, rt: tokio::runtime::Handle) -> Self {
        let http = reqwest::Client::builder().user_agent("Sylva").timeout(std::time::Duration::from_secs(20)).build().expect("HTTP client");
        Self { ctx, http, rt, map: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// The image if it is ready; starts fetching it the first time it is asked for.
    pub fn get(&self, url: &str) -> Option<Arc<Anim>> {
        let mut map = self.map.lock().unwrap();
        match map.get(url) {
            Some(Slot::Ready(a)) => return Some(a.clone()),
            Some(_) => return None,
            None => {}
        }
        map.insert(url.to_string(), Slot::Loading);
        drop(map);
        let this = self.clone();
        let url = url.to_string();
        self.rt.spawn(async move {
            let bytes = match this.http.get(&url).send().await {
                Ok(r) if r.status().is_success() => r.bytes().await.ok(),
                _ => None,
            };
            let ctx = this.ctx.clone();
            let name = url.clone();
            let anim = match bytes {
                Some(b) => tokio::task::spawn_blocking(move || decode(&ctx, &name, &b)).await.ok().flatten(),
                None => None,
            };
            let slot = match anim {
                Some(a) => Slot::Ready(Arc::new(a)),
                None => Slot::Failed,
            };
            this.map.lock().unwrap().insert(url, slot);
            this.ctx.request_repaint();
        });
        None
    }

    /// True once fetching or decoding the image has failed, so a fallback can show instead.
    pub fn failed(&self, url: &str) -> bool {
        matches!(self.map.lock().unwrap().get(url), Some(Slot::Failed))
    }
}

pub(crate) fn to_color_image(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

fn delay_ms(d: image::Delay) -> u32 {
    let (n, den) = d.numer_denom_ms();
    let ms = n.checked_div(den).unwrap_or(100);
    // Browsers treat very short GIF delays as 100 ms; match them so emotes look the same.
    if ms <= 10 { 100 } else { ms }
}

fn decode(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<Anim> {
    let format = image::guess_format(bytes).ok()?;
    let frames: Vec<(image::RgbaImage, u32)> = match format {
        ImageFormat::Gif => {
            let d = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?;
            d.into_frames().take(MAX_FRAMES).filter_map(Result::ok).map(|f| {
                let ms = delay_ms(f.delay());
                (f.into_buffer(), ms)
            }).collect()
        }
        ImageFormat::WebP => {
            let d = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
            if d.has_animation() {
                d.into_frames().take(MAX_FRAMES).filter_map(Result::ok).map(|f| {
                    let ms = delay_ms(f.delay());
                    (f.into_buffer(), ms)
                }).collect()
            } else {
                vec![(image::DynamicImage::from_decoder(d).ok()?.to_rgba8(), 0)]
            }
        }
        _ => vec![(image::load_from_memory(bytes).ok()?.to_rgba8(), 0)],
    };
    let first = frames.first()?;
    let size = egui::vec2(first.0.width() as f32, first.0.height() as f32);
    let total_ms = frames.iter().map(|f| f.1 as u64).sum();
    let frames = frames
        .iter()
        .enumerate()
        .map(|(i, (img, ms))| (ctx.load_texture(format!("{name}#{i}"), to_color_image(img), TextureOptions::LINEAR), *ms))
        .collect();
    Some(Anim { frames, total_ms, size })
}

