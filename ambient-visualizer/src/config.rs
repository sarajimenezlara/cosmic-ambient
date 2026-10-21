//! Configuración en `~/.config/cosmic-ambient/visualizer.toml`. La escribe la app
//! `cosmic-ambient` al cambiar de fondo; se relee sola cuando cambia el archivo.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::Deserialize;

#[derive(Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub mostrar: bool,
    /// "abajo-izquierda", "abajo-derecha", "arriba-izquierda" o "arriba-derecha".
    pub esquina: String,
    /// "auto" (sacados del fondo) o "manual" (los dos de abajo).
    pub colores: String,
    pub color_base: String,
    pub color_punta: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mostrar: true,
            esquina: "abajo-izquierda".into(),
            colores: "auto".into(),
            color_base: "#931610".into(),
            color_punta: "#ff150a".into(),
        }
    }
}

impl Config {
    pub fn arriba(&self) -> bool {
        self.esquina.starts_with("arriba")
    }

    pub fn derecha(&self) -> bool {
        self.esquina.ends_with("derecha")
    }

    /// Colores elegidos a mano, si los hay y son válidos.
    pub fn manual(&self) -> Option<([f32; 3], [f32; 3])> {
        (self.colores == "manual").then_some(())?;
        Some((hex(&self.color_base)?, hex(&self.color_punta)?))
    }
}

pub fn hex(s: &str) -> Option<[f32; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
    Some([c(0)?, c(2)?, c(4)?])
}

fn path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var("HOME").ok()?).join(".config/cosmic-ambient/visualizer.toml"))
}

/// Vigila el archivo y devuelve la configuración nueva cuando cambia.
pub struct Watcher {
    mtime: Option<SystemTime>,
    checked: Option<std::time::Instant>,
}

impl Watcher {
    pub fn new() -> Self {
        Watcher { mtime: None, checked: None }
    }

    pub fn poll(&mut self) -> Option<Config> {
        if self.checked.is_some_and(|t| t.elapsed().as_millis() < 1000) {
            return None;
        }
        self.checked = Some(std::time::Instant::now());
        let path = path()?;
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if mtime == self.mtime {
            return None;
        }
        self.mtime = mtime;
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Some(toml::from_str(&text).unwrap_or_default())
    }
}
