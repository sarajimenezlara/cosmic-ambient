//! Canción actual (vía `playerctl`/MPRIS) y su portada.

use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tiny_skia::{Pixmap, PremultipliedColorU8};

#[derive(Clone, Default, PartialEq)]
pub struct Track {
    pub playing: bool,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub art_url: String,
    pub length_us: u64,
}

pub struct NowPlaying {
    pub track: Option<Track>,
    pub position_us: u64,
    pub polled_at: Instant,
    pub art: Option<Pixmap>,
    /// Aumenta cada vez que cambia algo visible.
    pub version: u64,
    art_for: String,
}

impl NowPlaying {
    /// Posición estimada ahora mismo (se consulta una vez por segundo).
    pub fn position_now(&self) -> u64 {
        match &self.track {
            Some(t) if t.playing => {
                let extra = self.polled_at.elapsed().as_micros() as u64;
                (self.position_us + extra).min(t.length_us.max(self.position_us))
            }
            _ => self.position_us,
        }
    }
}

const SEP: char = '\u{1f}';

/// Elige qué reproductor mostrar: el que está sonando; si no suena ninguno,
/// el último que sonó; y si no, mejor un reproductor de música que un
/// navegador. `last_playing` guarda el nombre del último que sonó.
fn poll(last_playing: &mut Option<String>) -> Option<(Track, u64)> {
    let format = ["{{playerName}}", "{{status}}", "{{artist}}", "{{title}}", "{{album}}", "{{mpris:artUrl}}",
        "{{position}}", "{{mpris:length}}"]
        .join(&SEP.to_string());
    let out = Command::new("playerctl")
        .args(["-a", "metadata", "--format", &format])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);

    let mut best: Option<(u8, String, Track, u64)> = None;
    for line in text.lines() {
        let f: Vec<&str> = line.split(SEP).collect();
        if f.len() < 8 || f[3].trim().is_empty() || f[1] == "Stopped" {
            continue;
        }
        let name = f[0];
        let track = Track {
            playing: f[1] == "Playing",
            artist: f[2].to_string(),
            title: f[3].to_string(),
            album: f[4].to_string(),
            art_url: f[5].to_string(),
            length_us: f[7].parse().unwrap_or(0),
        };
        let pos = f[6].parse().unwrap_or(0);
        let browser = ["chromium", "chrome", "firefox", "opera", "brave", "vivaldi"]
            .iter()
            .any(|b| name.to_lowercase().contains(b));
        let rank = if track.playing {
            3
        } else if last_playing.as_deref() == Some(name) {
            2
        } else if !browser {
            1
        } else {
            0
        };
        if best.as_ref().is_none_or(|(r, ..)| rank > *r) {
            best = Some((rank, name.to_string(), track, pos));
        }
    }

    let (_, name, track, pos) = best?;
    if track.playing {
        *last_playing = Some(name);
    }
    Some((track, pos))
}

pub fn spawn(art_size: u32) -> Arc<Mutex<NowPlaying>> {
    let shared = Arc::new(Mutex::new(NowPlaying {
        track: None,
        position_us: 0,
        polled_at: Instant::now(),
        art: None,
        version: 0,
        art_for: String::new(),
    }));

    let np = shared.clone();
    let mut last_playing = None;
    thread::spawn(move || loop {
        let polled = poll(&mut last_playing);
        let fetch_art = {
            let mut s = np.lock().unwrap();
            let (track, pos) = match polled {
                Some((t, p)) => (Some(t), p),
                None => (None, 0),
            };
            if s.track != track {
                s.version += 1;
            }
            s.track = track;
            s.position_us = pos;
            s.polled_at = Instant::now();

            let url = s.track.as_ref().map(|t| t.art_url.clone()).unwrap_or_default();
            if url != s.art_for {
                s.art_for = url.clone();
                Some(url)
            } else {
                None
            }
        };

        if let Some(url) = fetch_art {
            let np = np.clone();
            thread::spawn(move || {
                let loaded = load_art(&url, art_size);
                let mut s = np.lock().unwrap();
                if s.art_for == url {
                    s.art = loaded;
                    s.version += 1;
                }
            });
        }

        thread::sleep(Duration::from_secs(1));
    });

    shared
}

/// Las apps Flatpak (p. ej. Opera GX) guardan la carátula en su /tmp
/// privado, que desde fuera está en $XDG_RUNTIME_DIR/.flatpak/<app>/tmp.
fn read_local(path: &str) -> Option<Vec<u8>> {
    if let Ok(bytes) = std::fs::read(path) {
        return Some(bytes);
    }
    let rel = path.strip_prefix("/tmp/")?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    std::fs::read_dir(format!("{runtime}/.flatpak"))
        .ok()?
        .flatten()
        .find_map(|app| std::fs::read(app.path().join("tmp").join(rel)).ok())
}

/// Carátula cuadrada de `size` píxeles; las miniaturas de vídeo (apaisadas)
/// se recortan por el centro.
fn load_art(url: &str, size: u32) -> Option<Pixmap> {
    let bytes = if let Some(path) = url.strip_prefix("file://") {
        read_local(path)?
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let out = Command::new("curl")
            .args(["-sfL", "--max-time", "8", url])
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)?
    } else {
        return None;
    };

    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let side = img.width().min(img.height());
    let square = image::imageops::crop_imm(
        &img,
        (img.width() - side) / 2,
        (img.height() - side) / 2,
        side,
        side,
    )
    .to_image();

    let resized = image::imageops::resize(&square, size, size, image::imageops::FilterType::CatmullRom);
    let mut pixmap = Pixmap::new(size, size)?;
    for (dst, src) in pixmap.pixels_mut().iter_mut().zip(resized.pixels()) {
        let [r, g, b, a] = src.0;
        let pm = |c: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
        *dst = PremultipliedColorU8::from_rgba(pm(r), pm(g), pm(b), a).unwrap();
    }
    Some(pixmap)
}
