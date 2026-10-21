//! Colores sacados del fondo de escritorio de COSMIC, para que el
//! visualizador vaya a juego. Se recalculan si cambias de fondo.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette {
    /// Color de la base de las barras.
    pub low: [f32; 3],
    /// Color de la punta de las barras y de los detalles.
    pub high: [f32; 3],
}

pub const DEFAULT: Palette = Palette { low: [0.30, 0.42, 0.85], high: [0.62, 0.78, 1.0] };

/// Ruta de la imagen de fondo según la config de cosmic-bg.
pub fn wallpaper_path() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var("HOME").ok()?)
        .join(".config/cosmic/com.system76.CosmicBackground/v1");
    let same_on_all = std::fs::read_to_string(dir.join("same-on-all"))
        .is_ok_and(|s| s.trim() == "true");

    let mut candidates = Vec::new();
    if !same_on_all {
        if let Ok(list) = std::fs::read_to_string(dir.join("backgrounds")) {
            for name in list.split('"').skip(1).step_by(2) {
                candidates.push(dir.join(format!("output.{name}")));
            }
        }
    }
    candidates.push(dir.join("all"));

    candidates.iter().find_map(|file| {
        let text = std::fs::read_to_string(file).ok()?;
        let rest = &text[text.find("Path(\"")? + 6..];
        let path = PathBuf::from(&rest[..rest.find('"')?]);
        path.is_file().then_some(path)
    })
}

pub fn from_image(path: &PathBuf) -> Option<Palette> {
    let img = image::open(path).ok()?.thumbnail(96, 96).to_rgba8();

    // Histograma de tonos ponderado por lo vivo que es cada píxel. El peso
    // crece muy rápido con el croma para que manden los colores intensos
    // (brillos, labios...) y no los tonos apagados como la piel.
    const BINS: usize = 24;
    let mut weight = [0.0f32; BINS];
    let mut sums = [[0.0f32; 3]; BINS];
    for p in img.pixels() {
        let rgb = [p[0], p[1], p[2]].map(|c| c as f32 / 255.0);
        let (h, s, l) = rgb_to_hsl(rgb);
        let vivid = s * (1.0 - (2.0 * l - 1.0).abs()); // croma
        if vivid < 0.3 {
            continue;
        }
        let bin = (h / 360.0 * BINS as f32) as usize % BINS;
        let w = vivid.powi(4);
        weight[bin] += w;
        for c in 0..3 {
            sums[bin][c] += rgb[c] * w;
        }
    }

    let avg = |b: usize| sums[b].map(|c| c / weight[b]);
    let main = (0..BINS).max_by(|a, b| weight[*a].total_cmp(&weight[*b]))?;
    if weight[main] <= 0.0 {
        return None;
    }
    let (h, s, _) = rgb_to_hsl(avg(main));
    let s = s.max(0.6);

    // Segundo tono: el más presente que esté lejos del principal en la rueda.
    let second = (0..BINS)
        .filter(|b| {
            let d = (*b as i32 - main as i32).rem_euclid(BINS as i32);
            d.min(BINS as i32 - d) >= 4 && weight[*b] >= weight[main] * 0.12
        })
        .max_by(|a, b| weight[*a].total_cmp(&weight[*b]));

    let (low, high) = match second {
        Some(b) => {
            let (h2, s2, _) = rgb_to_hsl(avg(b));
            (hsl_to_rgb(h, s.max(0.8), 0.32), hsl_to_rgb(h2, s2.max(0.55), 0.68))
        }
        // Una sola gama (p. ej. rojo y negro): el tono sale de las zonas oscuras
        // de ese color (sombras, labios), que es el más puro; mezclarlo con los
        // brillos y la piel lo vuelve naranja. La punta es ese tono, luminoso.
        None => {
            let (mut sum, mut n) = ([0.0f32; 3], 0.0);
            for p in img.pixels() {
                let rgb = [p[0], p[1], p[2]].map(|c| c as f32 / 255.0);
                let (ph, ps, pl) = rgb_to_hsl(rgb);
                let chroma = ps * (1.0 - (2.0 * pl - 1.0).abs());
                let dist = (ph - h).abs().min(360.0 - (ph - h).abs());
                if chroma >= 0.3 && dist <= 30.0 && pl < 0.4 {
                    for i in 0..3 {
                        sum[i] += rgb[i];
                    }
                    n += 1.0;
                }
            }
            let (dh, ds, _) = if n > 0.0 { rgb_to_hsl(sum.map(|c| c / n)) } else { (h, s, 0.0) };
            (hsl_to_rgb(dh, ds.max(0.8), 0.32), hsl_to_rgb(dh, 1.0, 0.52))
        }
    };
    Some(Palette { low, high })
}

pub fn spawn() -> Arc<Mutex<Palette>> {
    let shared = Arc::new(Mutex::new(DEFAULT));
    let out = shared.clone();
    thread::spawn(move || {
        let mut seen: Option<(PathBuf, Option<SystemTime>)> = None;
        loop {
            if let Some(path) = wallpaper_path() {
                let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let key = Some((path.clone(), mtime));
                if key != seen {
                    seen = key;
                    *out.lock().unwrap() = from_image(&path).unwrap_or(DEFAULT);
                }
            }
            thread::sleep(Duration::from_secs(5));
        }
    });
    shared
}

fn rgb_to_hsl([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s.min(1.0), l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}
