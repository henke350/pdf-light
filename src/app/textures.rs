//! Page images on the GPU, kept under a memory budget. The page view and the
//! pages panel each keep their own cache.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use pdf_light::error::PdfError;
use pdf_light::render::RenderedPage;
use pdf_light::render_pool::RenderRequest;
use std::collections::HashMap;

/// Cache key: original page index and the render scale's bits (an `f32` can't be a hash key).
pub type TextureKey = (usize, u32);

pub fn texture_key(page: usize, scale: f32) -> TextureKey {
    (page, scale.to_bits())
}

pub struct CachedTexture {
    texture: TextureHandle,
    scale: f32,
    bytes: usize,
    /// Frame number when it was last drawn, for least-recently-used eviction.
    last_used: u64,
}

/// Turns a finished render into a texture, or `None` when it failed or is
/// unusable (empty, or larger than the GPU allows).
pub fn load(
    ctx: &egui::Context,
    request: RenderRequest,
    result: Result<RenderedPage, PdfError>,
    frame: u64,
) -> Option<CachedTexture> {
    let max_side = ctx.input(|i| i.max_texture_side);
    let image = result.ok().filter(|img| {
        let (width, height) = (img.width as usize, img.height as usize);
        width > 0
            && height > 0
            && width <= max_side
            && height <= max_side
            && img.rgba.len() == width * height * 4
    })?;
    let size = [image.width as usize, image.height as usize];
    let kind = if request.thumbnail { "thumb" } else { "page" };
    let texture = ctx.load_texture(
        format!("{kind}-{}", request.page),
        ColorImage::from_rgba_unmultiplied(size, &image.rgba),
        TextureOptions::LINEAR,
    );
    Some(CachedTexture {
        texture,
        scale: request.scale,
        bytes: image.rgba.len(),
        last_used: frame,
    })
}

/// Page textures by (page, scale), kept under a memory budget.
#[derive(Default)]
pub struct TextureCache {
    entries: HashMap<TextureKey, CachedTexture>,
    total_bytes: usize,
}

impl TextureCache {
    pub fn insert(&mut self, key: TextureKey, entry: CachedTexture) {
        self.total_bytes += entry.bytes;
        if let Some(old) = self.entries.insert(key, entry) {
            self.total_bytes -= old.bytes;
        }
    }

    pub fn contains(&self, key: TextureKey) -> bool {
        self.entries.contains_key(&key)
    }

    pub fn has_any(&self, page: usize) -> bool {
        self.entries.keys().any(|&(p, _)| p == page)
    }

    /// The texture of `page` closest to `scale` (the exact one if cached), marked as used.
    pub fn best(&mut self, page: usize, scale: f32, frame: u64) -> Option<&TextureHandle> {
        let distance = |entry: &CachedTexture| (entry.scale / scale).ln().abs();
        let key = self
            .entries
            .iter()
            .filter(|&(&(p, _), _)| p == page)
            .min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))
            .map(|(&key, _)| key)?;
        let entry = self.entries.get_mut(&key)?;
        entry.last_used = frame;
        Some(&entry.texture)
    }

    /// Drops least-recently-used textures until under `budget`; textures drawn
    /// in `frame` stay. Dropping a `TextureHandle` frees the GPU texture.
    pub fn evict(&mut self, budget: usize, frame: u64) {
        while self.total_bytes > budget {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.last_used < frame)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(&key, _)| key);
            let Some(entry) = oldest.and_then(|key| self.entries.remove(&key)) else {
                break;
            };
            self.total_bytes -= entry.bytes;
        }
    }
}
