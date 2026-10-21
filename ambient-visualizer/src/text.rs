//! Texto con ab_glyph directamente sobre el Pixmap.

use std::process::Command;

use ab_glyph::{Font, FontVec, Glyph, PxScale, ScaleFont};
use tiny_skia::{Pixmap, PremultipliedColorU8};

pub struct Fonts {
    pub bold: FontVec,
    pub regular: FontVec,
    fallbacks: Vec<FontVec>,
}

fn load(pattern: &str) -> Option<FontVec> {
    let out = Command::new("fc-match").args(["-f", "%{file}|%{index}", pattern]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let (file, index) = text.split_once('|')?;
    let data = std::fs::read(file).ok()?;
    FontVec::try_from_vec_and_index(data, index.parse().unwrap_or(0)).ok()
}

impl Fonts {
    pub fn load() -> Fonts {
        let regular = load("sans").expect("no se encontró ninguna fuente (fc-match sans)");
        let bold = load("sans:bold").unwrap_or_else(|| load("sans").unwrap());
        // Para títulos con caracteres japoneses, coreanos, etc.
        let fallbacks = ["Noto Sans CJK JP", "DejaVu Sans"].iter().filter_map(|p| load(p)).collect();
        Fonts { bold, regular, fallbacks }
    }
}

struct Laid {
    font: usize,
    glyph: Glyph,
}

impl Fonts {
    fn pick<'a>(&'a self, main: &'a FontVec, c: char) -> (usize, &'a FontVec) {
        if main.glyph_id(c).0 != 0 {
            return (0, main);
        }
        for (i, f) in self.fallbacks.iter().enumerate() {
            if f.glyph_id(c).0 != 0 {
                return (i + 1, f);
            }
        }
        (0, main)
    }

    fn font<'a>(&'a self, main: &'a FontVec, idx: usize) -> &'a FontVec {
        if idx == 0 { main } else { &self.fallbacks[idx - 1] }
    }

    fn layout(&self, main: &FontVec, size: f32, text: &str, x: f32, baseline: f32) -> (Vec<Laid>, f32) {
        let scale = PxScale::from(size);
        let mut out: Vec<Laid> = Vec::new();
        let mut caret = x;
        for c in text.chars() {
            let (idx, font) = self.pick(main, c);
            let sf = font.as_scaled(scale);
            let id = font.glyph_id(c);
            if let Some(prev) = out.last() {
                if prev.font == idx {
                    caret += sf.kern(prev.glyph.id, id);
                }
            }
            out.push(Laid { font: idx, glyph: id.with_scale_and_position(scale, (caret, baseline)) });
            caret += sf.h_advance(id);
        }
        (out, caret - x)
    }

    pub fn measure(&self, main: &FontVec, size: f32, text: &str) -> f32 {
        self.layout(main, size, text, 0.0, 0.0).1
    }

    /// `text` recortado con "…" si no cabe en `max_w`.
    pub fn fit(&self, main: &FontVec, size: f32, text: &str, max_w: f32) -> String {
        let mut s: String = text.to_string();
        if self.measure(main, size, &s) > max_w {
            let ell = self.measure(main, size, "…");
            while !s.is_empty() && self.measure(main, size, &s) + ell > max_w {
                s.pop();
            }
            s = format!("{}…", s.trim_end());
        }
        s
    }

    /// Dibuja `text` (ya recortado con `fit` si hace falta) desde `x`.
    pub fn draw(
        &self,
        pix: &mut Pixmap,
        main: &FontVec,
        size: f32,
        x: f32,
        baseline: f32,
        text: &str,
        rgba: [f32; 4],
    ) {
        let (glyphs, _) = self.layout(main, size, text, x, baseline);
        let (w, h) = (pix.width() as i32, pix.height() as i32);
        let pixels = pix.pixels_mut();
        for laid in glyphs {
            let Some(outline) = self.font(main, laid.font).outline_glyph(laid.glyph) else {
                continue;
            };
            let b = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let px = b.min.x as i32 + gx as i32;
                let py = b.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= w || py >= h {
                    return;
                }
                let dst = &mut pixels[(py * w + px) as usize];
                let a = rgba[3] * cov.min(1.0);
                let inv = 1.0 - a;
                let ch = |src: f32, d: u8| (src * a * 255.0 + d as f32 * inv).round() as u8;
                let out_a = (a * 255.0 + dst.alpha() as f32 * inv).round() as u8;
                if let Some(c) = PremultipliedColorU8::from_rgba(
                    ch(rgba[0], dst.red()).min(out_a),
                    ch(rgba[1], dst.green()).min(out_a),
                    ch(rgba[2], dst.blue()).min(out_a),
                    out_a,
                ) {
                    *dst = c;
                }
            });
        }
    }
}
