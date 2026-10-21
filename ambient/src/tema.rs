//! La estética de cada fondo: una paleta de 26 colores con los nombres de
//! Catppuccin (rosewater … crust) que se lleva al escritorio: el tema de
//! COSMIC (ventanas, panel, la terminal y las apps GTK) y, si se activan en
//! `~/.config/cosmic-ambient/integraciones.toml`, el prompt de starship, el
//! resaltado de zsh y fzf, bat, btop, VS Code y Neovim.
//!
//! Las plantillas (`templates/`) son las de Catppuccin Mocha: cada color suyo
//! se cambia por el de la misma posición en la paleta del fondo.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use cosmic::cosmic_config::CosmicConfigEntry;
use cosmic::cosmic_theme::palette::{Srgb, Srgba};
use cosmic::cosmic_theme::{Theme, ThemeBuilder};
use serde::{Deserialize, Serialize};

/// Nombres de los colores, de los vivos a los fondos.
pub const COLORES: [&str; 26] = [
    "rosewater", "flamingo", "pink", "mauve", "red", "maroon", "peach", "yellow", "green", "teal",
    "sky", "sapphire", "blue", "lavender", "text", "subtext1", "subtext0", "overlay2", "overlay1",
    "overlay0", "surface2", "surface1", "surface0", "base", "mantle", "crust",
];

/// Catppuccin Mocha, en el mismo orden: con ella están hechas las plantillas
/// y es la paleta de los colores que falten.
const ORIGEN: [&str; 26] = [
    "#f5e0dc", "#f2cdcd", "#f5c2e7", "#cba6f7", "#f38ba8", "#eba0ac", "#fab387", "#f9e2af",
    "#a6e3a1", "#94e2d5", "#89dceb", "#74c7ec", "#89b4fa", "#b4befe", "#cdd6f4", "#bac2de",
    "#a6adc8", "#9399b2", "#7f849c", "#6c7086", "#585b70", "#45475a", "#313244", "#1e1e2e",
    "#181825", "#11111b",
];

const PLANTILLA_BAT: &str = include_str!("../templates/bat.tmTheme");
const PLANTILLA_BTOP: &str = include_str!("../templates/btop.theme");
const PLANTILLA_ZSH: &str = include_str!("../templates/zsh-syntax.zsh");

/// Qué más se tiñe aparte de COSMIC. Todo lo que no sea COSMIC viene apagado:
/// toca archivos de configuración de otros programas.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Integraciones {
    pub cosmic: bool,
    pub zsh: bool,
    pub starship: bool,
    pub bat: bool,
    pub btop: bool,
    pub vscode: bool,
    pub neovim: bool,
}

impl Default for Integraciones {
    fn default() -> Self {
        Integraciones { cosmic: true, zsh: false, starship: false, bat: false, btop: false, vscode: false, neovim: false }
    }
}

fn integraciones_path() -> PathBuf {
    home().join(".config/cosmic-ambient/integraciones.toml")
}

/// Lee las integraciones; la primera vez deja el archivo escrito para que se
/// vea qué se puede activar.
pub fn integraciones() -> Integraciones {
    let ruta = integraciones_path();
    match fs::read_to_string(&ruta) {
        Ok(t) => toml::from_str(&t).unwrap_or_default(),
        Err(_) => {
            let i = Integraciones::default();
            if let Ok(t) = toml::to_string_pretty(&i) {
                let _ = escribir(&ruta, &t);
            }
            i
        }
    }
}

/// Los colores que se pueden tocar desde la ventana (el resto, en perfil.toml).
pub const PRINCIPALES: [&str; 4] = ["mauve", "pink", "base", "text"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Tema {
    /// Sin esto, cambiar a este fondo no toca la estética del sistema.
    pub aplicar: bool,
    /// Nombre de Catppuccin → "#rrggbb". Los que falten salen de Catppuccin Mocha.
    pub colores: BTreeMap<String, String>,
}

impl Tema {
    pub fn color(&self, nombre: &str) -> String {
        match self.colores.get(nombre) {
            Some(c) if crate::store::es_hex(c) => c.trim().to_lowercase(),
            _ => ORIGEN[COLORES.iter().position(|n| *n == nombre).unwrap_or(14)].into(),
        }
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    (c(0), c(2), c(4))
}

fn srgb(hex: &str) -> Srgb {
    let (r, g, b) = rgb(hex);
    Srgb::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

fn srgba(hex: &str) -> Srgba {
    let c = srgb(hex);
    Srgba::new(c.red, c.green, c.blue, 1.0)
}

/// Cambia cada color de Catppuccin Mocha en `texto` por el de la misma posición en el tema.
fn recolorear(texto: &str, tema: &Tema) -> String {
    let destino: Vec<String> = COLORES.iter().map(|n| tema.color(n)).collect();
    let mut out = String::with_capacity(texto.len());
    let mut resto = texto;
    while let Some(i) = resto.find('#') {
        out.push_str(&resto[..i]);
        let candidato = resto.get(i..i + 7).unwrap_or("");
        let hex = candidato.len() == 7 && candidato[1..].chars().all(|c| c.is_ascii_hexdigit());
        match ORIGEN.iter().position(|o| hex && o.eq_ignore_ascii_case(candidato)) {
            Some(k) => {
                // Respeta mayúsculas si la plantilla las usa.
                let nuevo = &destino[k];
                if candidato[1..].chars().any(|c| c.is_ascii_uppercase()) {
                    out.push_str(&nuevo.to_uppercase());
                } else {
                    out.push_str(nuevo);
                }
                resto = &resto[i + 7..];
            }
            None => {
                out.push('#');
                resto = &resto[i + 1..];
            }
        }
    }
    out.push_str(resto);
    out
}

/// Escribe solo si cambia, para no despertar a quien vigila el archivo.
fn escribir(ruta: &PathBuf, texto: &str) -> io::Result<()> {
    if fs::read_to_string(ruta).is_ok_and(|t| t == texto) {
        return Ok(());
    }
    fs::create_dir_all(ruta.parent().unwrap())?;
    fs::write(ruta, texto)
}

/// Pone la estética del tema en todo. Devuelve avisos de lo que falló.
/// Sin `forzar`, no hace nada si ya está puesto este mismo tema (al retocar
/// otras cosas del fondo no hace falta repintar todo el escritorio).
pub fn aplicar(tema: &Tema, nombre: &str, forzar: bool) -> Vec<String> {
    if let Some((n, c)) = tema.colores.iter().find(|(_, c)| !crate::store::es_hex(c)) {
        return vec![format!("estética: «{c}» no es un color #rrggbb ({n})")];
    }
    let int = integraciones();
    let marca = home().join(".local/state/cosmic-ambient/tema-puesto");
    let firma = format!("{nombre}\n{:?}\n{int:?}", tema.colores);
    if !forzar && fs::read_to_string(&marca).is_ok_and(|m| m == firma) {
        return Vec::new();
    }
    let mut avisos = Vec::new();
    let pasos: [(bool, &str, fn(&Tema, &str) -> io::Result<()>); 7] = [
        (int.cosmic, "tema de COSMIC", cosmic),
        (int.zsh, "zsh y fzf", zsh),
        (int.starship, "starship", starship),
        (int.bat, "bat", bat),
        (int.btop, "btop", btop),
        (int.vscode, "VS Code", vscode),
        (int.neovim, "Neovim", neovim),
    ];
    for (que, paso) in pasos.into_iter().filter(|p| p.0).map(|p| (p.1, p.2)) {
        if let Err(e) = paso(tema, nombre) {
            avisos.push(format!("{que}: {e}"));
        }
    }
    if avisos.is_empty() {
        let _ = escribir(&marca, &firma);
    }
    avisos
}

/// Tema oscuro de COSMIC (como lo deja Ajustes → Apariencia) y su copia para GTK.
/// La terminal de COSMIC usa este tema, así que cambia con él.
fn cosmic(t: &Tema, nombre: &str) -> io::Result<()> {
    let config = ThemeBuilder::dark_config().map_err(io::Error::other)?;
    let mut b = ThemeBuilder::get_entry(&config).unwrap_or_else(|(_, b)| b);
    let c = |n: &str| srgba(&t.color(n));
    {
        let p = b.palette.as_mut();
        p.name = nombre.into();
        p.bright_red = c("red");
        p.bright_green = c("green");
        p.bright_orange = c("peach");
        p.gray_1 = c("mantle");
        p.gray_2 = c("base");
        let neutros = [
            "crust", "mantle", "base", "surface0", "surface1", "surface2", "overlay0", "overlay1",
            "overlay2", "subtext0", "subtext1",
        ];
        let campos = [
            &mut p.neutral_0, &mut p.neutral_1, &mut p.neutral_2, &mut p.neutral_3, &mut p.neutral_4,
            &mut p.neutral_5, &mut p.neutral_6, &mut p.neutral_7, &mut p.neutral_8, &mut p.neutral_9,
            &mut p.neutral_10,
        ];
        for (campo, n) in campos.into_iter().zip(neutros) {
            *campo = c(n);
        }
        p.accent_blue = c("blue");
        p.accent_indigo = c("mauve");
        p.accent_purple = c("lavender");
        p.accent_pink = c("pink");
        p.accent_red = c("red");
        p.accent_orange = c("peach");
        p.accent_yellow = c("yellow");
        p.accent_green = c("green");
        p.accent_warm_grey = c("overlay2");
        p.ext_warm_grey = c("overlay2");
        p.ext_orange = c("peach");
        p.ext_yellow = c("yellow");
        p.ext_blue = c("blue");
        p.ext_purple = c("lavender");
        p.ext_pink = c("pink");
        p.ext_indigo = c("mauve");
    }
    let s = |n: &str| Some(srgb(&t.color(n)));
    b.bg_color = Some(c("base"));
    b.primary_container_bg = Some(c("surface0"));
    b.secondary_container_bg = Some(c("surface1"));
    b.text_tint = s("text");
    b.neutral_tint = s("overlay1");
    b.accent = s("mauve");
    b.window_hint = s("mauve");
    b.success = s("green");
    b.warning = s("yellow");
    b.destructive = s("red");
    b.write_entry(&config).map_err(io::Error::other)?;

    let theme = b.build();
    theme.write_entry(&Theme::dark_config().map_err(io::Error::other)?).map_err(io::Error::other)?;
    theme.write_gtk4().map_err(io::Error::other)
}

/// `~/.config/cosmic-ambient/theme.zsh`: resaltado, sugerencias y fzf. Ver
/// `extras/zshrc.zsh` para cargarlo (y recargarlo solo) desde el .zshrc.
fn zsh(t: &Tema, _: &str) -> io::Result<()> {
    let base = recolorear(PLANTILLA_ZSH, t);
    let c = |n: &str| t.color(n);
    let texto = format!(
        "# Generado por cosmic-ambient: no editar a mano.\n{base}\n\
         ZSH_AUTOSUGGEST_HIGHLIGHT_STYLE='fg={}'\n\
         export FZF_DEFAULT_OPTS=\" \\\n\
         --color=bg+:{},bg:{},spinner:{},hl:{} \\\n\
         --color=fg:{},header:{},info:{},pointer:{} \\\n\
         --color=marker:{},fg+:{},prompt:{},hl+:{} \\\n\
         --border=rounded --layout=reverse --height=50%\"\n",
        c("overlay0"),
        c("surface0"), c("base"), c("rosewater"), c("red"),
        c("text"), c("red"), c("mauve"), c("rosewater"),
        c("lavender"), c("text"), c("mauve"), c("red"),
    );
    escribir(&home().join(".config/cosmic-ambient/theme.zsh"), &texto)
}

/// Paleta `ambient` de starship, con los nombres de Catppuccin (se lee en cada prompt).
fn starship(t: &Tema, _: &str) -> io::Result<()> {
    let ruta = home().join(".config/starship.toml");
    let texto = fs::read_to_string(&ruta).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = texto.parse().map_err(io::Error::other)?;
    doc["palette"] = toml_edit::value("ambient");
    let mut tabla = toml_edit::Table::new();
    for n in COLORES {
        tabla[n] = toml_edit::value(t.color(n));
    }
    if !doc.contains_table("palettes") {
        let mut p = toml_edit::Table::new();
        p.set_implicit(true);
        doc["palettes"] = toml_edit::Item::Table(p);
    }
    doc["palettes"]["ambient"] = toml_edit::Item::Table(tabla);
    escribir(&ruta, &doc.to_string())
}

/// Tema «Ambient» de bat (la caché de temas hay que rehacerla).
fn bat(t: &Tema, _: &str) -> io::Result<()> {
    let tm = recolorear(PLANTILLA_BAT, t)
        .replace("<string>Catppuccin Mocha</string>", "<string>Ambient</string>");
    escribir(&home().join(".config/bat/themes/Ambient.tmTheme"), &tm)?;
    let config = home().join(".config/bat/config");
    let actual = fs::read_to_string(&config).unwrap_or_default();
    let mut lineas: Vec<String> =
        actual.lines().filter(|l| !l.trim_start().starts_with("--theme")).map(String::from).collect();
    lineas.insert(0, "--theme=\"Ambient\"".into());
    escribir(&config, &(lineas.join("\n") + "\n"))?;
    // En Debian/Ubuntu el ejecutable se llama batcat.
    let cache = |bin: &str| {
        Command::new(bin).args(["cache", "--build"]).stdout(Stdio::null()).stderr(Stdio::null()).status()
    };
    cache("bat").or_else(|_| cache("batcat"))?;
    Ok(())
}

/// Tema «ambient» de btop (lo coge al abrirse).
fn btop(t: &Tema, _: &str) -> io::Result<()> {
    let tema = home().join(".config/btop/themes/ambient.theme");
    escribir(&tema, &recolorear(PLANTILLA_BTOP, t))?;
    let conf = home().join(".config/btop/btop.conf");
    let actual = fs::read_to_string(&conf).unwrap_or_default();
    let mut lineas: Vec<String> =
        actual.lines().filter(|l| !l.trim_start().starts_with("color_theme")).map(String::from).collect();
    lineas.insert(0, format!("color_theme = \"{}\"", tema.display()));
    escribir(&conf, &(lineas.join("\n") + "\n"))
}

/// VS Code con la extensión Catppuccin: Mocha con los colores cambiados; se recarga solo.
fn vscode(t: &Tema, _: &str) -> io::Result<()> {
    let ruta = home().join(".config/Code/User/settings.json");
    let texto = fs::read_to_string(&ruta)?;
    let mut json: serde_json::Value = serde_json::from_str(&texto).map_err(io::Error::other)?;
    let mocha: serde_json::Map<String, serde_json::Value> =
        COLORES.iter().map(|n| (n.to_string(), t.color(n).into())).collect();
    json["catppuccin.colorOverrides"] = serde_json::json!({ "mocha": mocha });

    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    json.serialize(&mut ser).map_err(io::Error::other)?;
    escribir(&ruta, &String::from_utf8_lossy(&out))
}

/// `~/.local/state/cosmic-ambient/palette.lua`: la lee Catppuccin en Neovim
/// (ver `extras/nvim/ambient.lua`), que vigila el archivo y se recolorea solo.
fn neovim(t: &Tema, _: &str) -> io::Result<()> {
    let mut lua = String::from("-- Generado por cosmic-ambient: paleta del fondo puesto.\nreturn {\n");
    for n in COLORES {
        lua.push_str(&format!("  {n} = \"{}\",\n", t.color(n)));
    }
    lua.push_str("}\n");
    escribir(&home().join(".local/state/cosmic-ambient/palette.lua"), &lua)
}
