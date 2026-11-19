//! Text with positions, read from a page's content stream.

use hayro::hayro_syntax::Pdf;
use hayro_interpret::font::{Glyph, GlyphRun};
use hayro_interpret::hayro_cmap::BfString;
use hayro_interpret::kurbo::{BezPath, Point, Rect, Shape};
use hayro_interpret::util::TransformExt;
use hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, interpret_page,
};

/// A rectangle on a page, in points, origin top-left, y pointing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// One drawn glyph: its Unicode text and where it sits on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct TextGlyph {
    pub text: String,
    pub rect: PageRect,
}

/// Glyph box in glyph units (1000 per em): below the baseline to above the
/// cap height, which gives a line-sized highlight.
const BOX_BOTTOM: f64 = -200.0;
const BOX_TOP: f64 = 800.0;
/// Width used when nothing better is known.
const FALLBACK_WIDTH: f64 = 500.0;

/// Returns the glyphs of page `index` in content-stream order. A page that
/// does not exist or has no text gives an empty list.
pub fn extract_glyphs(pdf: &Pdf, index: usize) -> Vec<TextGlyph> {
    let Some(page) = pdf.pages().get(index) else {
        return Vec::new();
    };
    let (w, h) = page.render_dimensions();
    let cache = InterpreterCache::new();
    // This transform flips y so the origin is top-left, like rendering at 1x.
    let mut ctx = Context::new(
        page.initial_transform(true).to_kurbo(),
        Rect::new(0.0, 0.0, f64::from(w), f64::from(h)),
        &cache,
        pdf.xref(),
        InterpreterSettings::default(),
    );
    let mut collector = TextCollector::default();
    interpret_page(page, &mut ctx, &mut collector);
    collector.glyphs
}

/// Collects glyphs while hayro "draws" a page. It only owns plain data
/// (Strings and numbers), so it does not depend on the PDF's lifetime.
#[derive(Default)]
struct TextCollector {
    glyphs: Vec<TextGlyph>,
}

// A trait is a list of methods a type promises to provide. By implementing
// `Device` we let hayro's interpreter call us for every drawing operation;
// we only care about glyphs, so the other methods do nothing.
impl<'a> Device<'a> for TextCollector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}

    fn push_clip_path(&mut self, _: &ClipPath) {}

    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        let glyphs = run.glyphs();
        for (i, glyph) in glyphs.iter().enumerate() {
            let Some(unicode) = glyph.as_unicode() else {
                continue;
            };
            let text = match unicode {
                BfString::Char(c) => c.to_string(),
                BfString::String(s) => s,
            };
            // Glyph space -> device space (page points, y down).
            let t = props.transform * glyph.transform();

            // Width: distance to the next glyph's origin, in glyph units.
            let next_gap = glyphs.get(i + 1).and_then(|next| {
                let origin = t.inverse() * (props.transform * next.transform() * Point::ZERO);
                (origin.x.is_finite() && origin.x > 0.0).then_some(origin.x)
            });
            let width = next_gap.unwrap_or_else(|| match &**glyph {
                Glyph::Type3(g) => g
                    .advance_width()
                    .map(f64::from)
                    .filter(|w| *w > 0.0)
                    .unwrap_or(FALLBACK_WIDTH),
                Glyph::Outline(g) => {
                    let x1 = g.outline().bounding_box().x1;
                    if x1 > 0.0 { x1 } else { FALLBACK_WIDTH }
                }
            });

            let r = t.transform_rect_bbox(Rect::new(0.0, BOX_BOTTOM, width, BOX_TOP));
            self.glyphs.push(TextGlyph {
                text,
                rect: PageRect {
                    x0: r.x0 as f32,
                    y0: r.y0 as f32,
                    x1: r.x1 as f32,
                    y1: r.y1 as f32,
                },
            });
        }
    }

    fn pop_clip(&mut self) {}

    fn pop_transparency_group(&mut self) {}

    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
}
