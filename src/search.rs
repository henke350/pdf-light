//! Turning glyphs into searchable page text, and finding matches in it.

use crate::text::{PageRect, TextGlyph};

/// Page text as lowercase chars, each tied to the rect it came from.
/// `chars` and `rects` always have the same length.
pub struct PageText {
    pub chars: Vec<char>,
    pub rects: Vec<PageRect>,
}

fn height(r: &PageRect) -> f32 {
    r.y1 - r.y0
}

fn centre_y(r: &PageRect) -> f32 {
    (r.y0 + r.y1) / 2.0
}

/// Flattens glyphs into text, inserting a space at line breaks and gaps.
pub fn build_page_text(glyphs: &[TextGlyph]) -> PageText {
    let mut chars: Vec<char> = Vec::new();
    let mut rects: Vec<PageRect> = Vec::new();
    let mut prev: Option<&TextGlyph> = None;
    for g in glyphs {
        if g.text.chars().all(char::is_whitespace) {
            if chars.last() != Some(&' ') {
                chars.push(' ');
                rects.push(g.rect);
            }
        } else {
            if let Some(p) = prev {
                let h = height(&g.rect);
                let new_line = (centre_y(&g.rect) - centre_y(&p.rect)).abs() > h / 2.0;
                let gap = g.rect.x0 - p.rect.x1 > 0.25 * h;
                if (new_line || gap) && chars.last() != Some(&' ') {
                    chars.push(' ');
                    rects.push(p.rect);
                }
            }
            // `flat_map` turns each char into its lowercase chars (some
            // expand to several); a ligature's chars all share one rect.
            for c in g.text.chars().flat_map(char::to_lowercase) {
                chars.push(c);
                rects.push(g.rect);
            }
        }
        prev = Some(g);
    }
    PageText { chars, rects }
}

/// Lowercases, collapses whitespace runs to one space and trims.
pub fn normalize_query(q: &str) -> Vec<char> {
    let lower = q.to_lowercase();
    // split_whitespace drops empty pieces, so joining gives single spaces.
    lower
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect()
}

/// Finds non-overlapping matches left to right. Each match is returned as
/// one merged rect per line.
pub fn find_in_page(text: &PageText, query: &[char]) -> Vec<Vec<PageRect>> {
    let mut matches = Vec::new();
    if query.is_empty() {
        return matches;
    }
    let mut start = 0;
    while start + query.len() <= text.chars.len() {
        if text.chars[start..start + query.len()] == *query {
            matches.push(merge_per_line(&text.rects[start..start + query.len()]));
            start += query.len();
        } else {
            start += 1;
        }
    }
    matches
}

/// Merges consecutive rects on the same line into one bounding rect.
fn merge_per_line(rects: &[PageRect]) -> Vec<PageRect> {
    let mut out: Vec<PageRect> = Vec::new();
    for r in rects {
        match out.last_mut() {
            Some(last) if (centre_y(r) - centre_y(last)).abs() <= height(last) / 2.0 => {
                last.x0 = last.x0.min(r.x0);
                last.y0 = last.y0.min(r.y0);
                last.x1 = last.x1.max(r.x1);
                last.y1 = last.y1.max(r.y1);
            }
            _ => out.push(*r),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(text: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> TextGlyph {
        TextGlyph {
            text: text.to_string(),
            rect: PageRect { x0, y0, x1, y1 },
        }
    }

    /// Lays out `s` as touching 5-wide, 10-high glyphs on one line at `y0`.
    fn line(s: &str, y0: f32) -> Vec<TextGlyph> {
        s.chars()
            .enumerate()
            .map(|(i, c)| {
                let x = i as f32 * 5.0;
                glyph(&c.to_string(), x, y0, x + 5.0, y0 + 10.0)
            })
            .collect()
    }

    fn chars_of(glyphs: &[TextGlyph]) -> String {
        build_page_text(glyphs).chars.into_iter().collect()
    }

    #[test]
    fn build_page_text_should_insert_space_for_horizontal_gap() {
        let g = [
            glyph("a", 0.0, 0.0, 5.0, 10.0),
            glyph("b", 20.0, 0.0, 25.0, 10.0),
        ];
        assert_eq!(chars_of(&g), "a b");
    }

    #[test]
    fn build_page_text_should_not_insert_space_between_touching_glyphs() {
        let g = [
            glyph("a", 0.0, 0.0, 5.0, 10.0),
            glyph("b", 5.0, 0.0, 10.0, 10.0),
        ];
        assert_eq!(chars_of(&g), "ab");
    }

    #[test]
    fn build_page_text_should_insert_space_at_new_line() {
        let g = [
            glyph("a", 0.0, 0.0, 5.0, 10.0),
            glyph("b", 0.0, 20.0, 5.0, 30.0),
        ];
        assert_eq!(chars_of(&g), "a b");
    }

    #[test]
    fn find_in_page_should_ignore_case() {
        let page = build_page_text(&line("Hello", 0.0));
        assert_eq!(find_in_page(&page, &normalize_query("hELLo")).len(), 1);
    }

    #[test]
    fn find_in_page_should_treat_whitespace_runs_as_one_space() {
        let page = build_page_text(&line("foo bar", 0.0));
        assert_eq!(find_in_page(&page, &normalize_query("foo   bar")).len(), 1);
    }

    #[test]
    fn find_in_page_should_map_ligature_to_one_rect() {
        let rect = PageRect {
            x0: 0.0,
            y0: 0.0,
            x1: 8.0,
            y1: 10.0,
        };
        let page = build_page_text(&[glyph("fi", 0.0, 0.0, 8.0, 10.0)]);
        let matches = find_in_page(&page, &normalize_query("fi"));
        assert_eq!(matches, vec![vec![rect]]);
    }

    #[test]
    fn find_in_page_should_split_match_rects_per_line() {
        let mut g = line("ab", 0.0);
        g.extend(line("cd", 20.0));
        let page = build_page_text(&g);
        let matches = find_in_page(&page, &normalize_query("b c"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].len(), 2);
    }

    #[test]
    fn find_in_page_should_return_nothing_for_empty_query() {
        let page = build_page_text(&line("abc", 0.0));
        assert!(find_in_page(&page, &normalize_query("   ")).is_empty());
    }
}
