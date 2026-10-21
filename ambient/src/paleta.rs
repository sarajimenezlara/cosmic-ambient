//! Paletas sacadas de la imagen del fondo. Se buscan los tonos vivos que más
//! pesan en la imagen; uno hace de acento (mauve) y otro de acento secundario
//! (pink), y los fondos y textos son los de Catppuccin Mocha teñidos con el
//! tono del acento. Los colores con significado (rojo de error, verde de
//! éxito…) se quedan con su tono de siempre.

use std::collections::BTreeMap;
use std::path::Path;

use crate::tema::{COLORES, ORIGEN};

/// Grupos de tono en la rueda de color (de 15° cada uno).
const GRUPOS: usize = 24;

/// Un tono vivo de la imagen y lo que pesa en ella.
struct Candidato {
    h: f32,
    s: f32,
    l: f32,
    peso: f32,
}

/// Paleta completa (los 26 colores) para la imagen. Cada `variante` es otra
/// propuesta: primero cambia el tono que hace de acento y, cuando se acaban,
/// el tinte de los fondos (del acento, del secundario o casi gris). Al
/// acabarse todas, vuelven a empezar.
pub fn desde_imagen(imagen: &Path, variante: usize) -> Option<BTreeMap<String, String>> {
    let img = image::open(imagen).ok()?.thumbnail(160, 160).to_rgb8();
    let pixeles: Vec<[f32; 3]> = img.pixels().map(|p| [p[0], p[1], p[2]].map(|c| c as f32 / 255.0)).collect();
    let candidatos = candidatos(&pixeles);
    Some(paleta(&candidatos, variante))
}

fn candidatos(pixeles: &[[f32; 3]]) -> Vec<Candidato> {
    // Histograma de tonos: cada píxel pesa lo vivo que es, para que un
    // detalle de color intenso cuente más que una gran zona apagada.
    let mut peso = [0.0f32; GRUPOS];
    let mut suma = [[0.0f32; 3]; GRUPOS];
    for &rgb in pixeles {
        let (h, s, l) = a_hsl(rgb);
        let croma = s * (1.0 - (2.0 * l - 1.0).abs());
        if croma < 0.06 {
            continue;
        }
        let g = (h / 360.0 * GRUPOS as f32) as usize % GRUPOS;
        let w = croma;
        peso[g] += w;
        for c in 0..3 {
            suma[g][c] += rgb[c] * w;
        }
    }

    // Picos del histograma, del que más pesa al que menos, sin dos tonos
    // vecinos y sin los que apenas aparecen.
    let total: f32 = peso.iter().sum();
    let mut orden: Vec<usize> = (0..GRUPOS).filter(|g| peso[*g] > total * 0.01).collect();
    orden.sort_by(|a, b| peso[*b].total_cmp(&peso[*a]));
    let mut elegidos: Vec<Candidato> = Vec::new();
    for g in orden {
        let (h, s, l) = a_hsl(suma[g].map(|c| c / peso[g]));
        if elegidos.iter().all(|c| distancia(c.h, h) >= 25.0) {
            elegidos.push(Candidato { h, s, l, peso: peso[g] });
        }
    }
    elegidos.truncate(6);
    elegidos
}

fn paleta(candidatos: &[Candidato], variante: usize) -> BTreeMap<String, String> {
    // Imagen sin color (blanco y negro): se gira el morado de Mocha.
    let n = candidatos.len().max(1);
    let tinte = variante / n % 3;
    let (acento, secundario) = if candidatos.is_empty() {
        let h = (267.0 + variante as f32 * 45.0) % 360.0;
        ((h, 0.8, 0.75), ((h + 50.0) % 360.0, 0.75, 0.8))
    } else {
        let a = &candidatos[variante % n];
        // El secundario: el tono que más pesa de los otros; si no hay, uno vecino.
        let b = candidatos
            .iter()
            .filter(|c| distancia(c.h, a.h) >= 25.0)
            .max_by(|x, y| x.peso.total_cmp(&y.peso))
            .map_or(((a.h + 330.0) % 360.0, a.s, a.l), |c| (c.h, c.s, c.l));
        ((a.h, a.s, a.l), b)
    };

    // Sobre fondo oscuro los acentos tienen que ser claros y bastante vivos.
    let vivo = |(h, s, l): (f32, f32, f32)| (h, s.clamp(0.5, 0.9), l.clamp(0.62, 0.78));
    let acento = vivo(acento);
    let secundario = vivo(secundario);
    let (ha, sa, la) = acento;
    // Tono y saturación de textos y fondos.
    let (hn, sn) = match tinte {
        0 => (ha, 1.0),
        1 => (secundario.0, 1.0),
        _ => (ha, 0.35),
    };

    let mut colores = BTreeMap::new();
    for (i, nombre) in COLORES.iter().enumerate() {
        let (h, s, l) = a_hsl(rgb(ORIGEN[i]));
        let hsl = match *nombre {
            "mauve" => acento,
            "pink" => secundario,
            "lavender" => (ha, sa * 0.8, (la + 0.08).min(0.85)),
            "rosewater" => (secundario.0, 0.6, 0.91),
            "flamingo" => (secundario.0, 0.6, 0.86),
            // Textos y fondos: los de Mocha con el tono del acento. Los fondos,
            // algo menos teñidos para que no cansen.
            "text" | "subtext1" | "subtext0" | "overlay2" | "overlay1" | "overlay0" => (hn, s * sn, l),
            "surface2" | "surface1" | "surface0" | "base" | "mantle" | "crust" => (hn, s * sn * 0.8, l),
            _ => (h, s, l),
        };
        colores.insert(nombre.to_string(), hex(de_hsl(hsl)));
    }
    colores
}

/// Distancia entre dos tonos en la rueda (0–180°).
fn distancia(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

fn rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
    [c(0), c(2), c(4)]
}

fn hex(rgb: [f32; 3]) -> String {
    let [r, g, b] = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn a_hsl([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

fn de_hsl((h, s, l): (f32, f32, f32)) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_ida_y_vuelta() {
        for c in ORIGEN {
            assert_eq!(hex(de_hsl(a_hsl(rgb(c)))), c);
        }
    }

    #[test]
    fn acento_del_color_que_manda() {
        // Una imagen casi toda naranja con un poco de azul.
        let mut pixeles = vec![[0.95, 0.5, 0.2]; 900];
        pixeles.extend(vec![[0.2, 0.3, 0.9]; 100]);
        let c = candidatos(&pixeles);
        let p = paleta(&c, 0);
        let (h, _, _) = a_hsl(rgb(&p["mauve"]));
        assert!(distancia(h, 21.0) < 10.0, "acento {}", p["mauve"]);
        let (h2, _, _) = a_hsl(rgb(&p["pink"]));
        assert!(distancia(h2, 228.0) < 10.0, "secundario {}", p["pink"]);
        // La otra propuesta pone el azul de acento.
        let (h, _, _) = a_hsl(rgb(&paleta(&c, 1)["mauve"]));
        assert!(distancia(h, 228.0) < 10.0);
        assert_eq!(p.len(), 26);
    }

    #[test]
    fn imagen_gris() {
        let p = paleta(&candidatos(&vec![[0.5, 0.5, 0.5]; 100]), 0);
        assert_eq!(p.len(), 26);
    }
}

