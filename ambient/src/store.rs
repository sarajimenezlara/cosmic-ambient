//! El almacén: `~/.local/share/cosmic-ambient/<id>/` con la imagen, una miniatura y
//! `perfil.toml`. Aplicar un perfil cambia el fondo de COSMIC, el modo
//! oscuro, el visualizador de música, el reloj y la estética del sistema
//! (ver `tema`).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

pub const ESQUINAS: [&str; 4] = ["abajo-izquierda", "abajo-derecha", "arriba-izquierda", "arriba-derecha"];
pub const POSICIONES_RELOJ: [&str; 9] = [
    "top left", "top center", "top right", "center left", "center", "center right",
    "bottom left", "bottom center", "bottom right",
];
pub const ESQUEMAS_RELOJ: [&str; 10] = [
    "blue", "pink", "green", "yellow", "red", "light-green", "purple", "dark-blue", "grey", "custom",
];
/// Colores del reloj cuando el esquema es "custom".
pub const COLORES_RELOJ: [&str; 7] = ["corner1", "corner2", "time", "date", "bottom", "shadow", "highlight"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Perfil {
    pub nombre: String,
    pub imagen: String,
    pub modo_oscuro: bool,
    pub visualizador: Visualizador,
    pub reloj: Reloj,
    pub tema: crate::tema::Tema,
}

/// Se escribe tal cual en `~/.config/cosmic-ambient/visualizer.toml`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Visualizador {
    pub mostrar: bool,
    pub esquina: String,
    /// "auto" (sacados del fondo) o "manual".
    pub colores: String,
    pub color_base: String,
    pub color_punta: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Reloj {
    pub mostrar: bool,
    /// "texto" (ambient-clock: letras con borde y sombra, o sobre una tarjeta)
    /// o "moonlight" (el Moonlight Clock, si está instalado, con su forma).
    pub estilo: String,
    pub posicion: String,
    pub x: i64,
    pub y: i64,
    /// Tamaño del reloj (1.0 = normal).
    pub escala: f64,
    pub esquema: String,
    pub colores: BTreeMap<String, String>,
    /// Formato de la hora del Moonlight (p. ej. "%H:%M"); sin él no se toca.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formato_hora: Option<String>,
    /// Tamaño de letra de la hora del Moonlight; sin él no se toca.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tamano_hora: Option<i64>,
    /// Solo para el estilo "texto".
    pub texto: RelojTexto,
}

/// Se escribe (junto con mostrar/posición/x/y/escala) en
/// `~/.config/cosmic-ambient/clock.toml`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct RelojTexto {
    pub fuente: String,
    pub color_relleno: String,
    pub color_borde: String,
    pub color_sombra: String,
    /// Tarjeta redondeada detrás del reloj; vacío = sin tarjeta.
    pub color_fondo: String,
    pub grosor_borde: f64,
    pub sombra: f64,
    pub formato_hora: String,
    pub formato_fecha: String,
}

impl Default for RelojTexto {
    fn default() -> Self {
        RelojTexto {
            fuente: "sans:bold".into(),
            color_relleno: "#ffffff".into(),
            color_borde: "#1e1e2e".into(),
            color_sombra: "#11111b".into(),
            color_fondo: String::new(),
            grosor_borde: 3.0,
            sombra: 4.0,
            formato_hora: "%H:%M".into(),
            formato_fecha: "%d / %m".into(),
        }
    }
}

impl Default for Perfil {
    fn default() -> Self {
        Perfil {
            nombre: String::new(),
            imagen: String::new(),
            modo_oscuro: true,
            visualizador: Visualizador::default(),
            reloj: Reloj::default(),
            tema: Default::default(),
        }
    }
}

impl Default for Visualizador {
    fn default() -> Self {
        Visualizador {
            mostrar: true,
            esquina: ESQUINAS[0].into(),
            colores: "auto".into(),
            color_base: "#931610".into(),
            color_punta: "#ff150a".into(),
        }
    }
}

impl Default for Reloj {
    fn default() -> Self {
        Reloj {
            mostrar: true,
            estilo: "texto".into(),
            posicion: "top right".into(),
            x: 40,
            y: 60,
            escala: 1.0,
            esquema: "blue".into(),
            colores: BTreeMap::new(),
            formato_hora: None,
            tamano_hora: None,
            texto: RelojTexto::default(),
        }
    }
}

pub struct Fondo {
    pub id: String,
    pub dir: PathBuf,
    pub perfil: Perfil,
}

impl Fondo {
    pub fn imagen(&self) -> PathBuf {
        self.dir.join(&self.perfil.imagen)
    }

    pub fn miniatura(&self) -> PathBuf {
        self.dir.join("miniatura.png")
    }

    pub fn guardar(&self) -> io::Result<()> {
        let text = toml::to_string_pretty(&self.perfil).map_err(io::Error::other)?;
        fs::write(self.dir.join("perfil.toml"), text)
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

pub fn raiz() -> PathBuf {
    home().join(".local/share/cosmic-ambient")
}

fn bg_dir() -> PathBuf {
    home().join(".config/cosmic/com.system76.CosmicBackground/v1")
}

fn visualizador_config() -> PathBuf {
    home().join(".config/cosmic-ambient/visualizer.toml")
}

/// Ejecutable de este proyecto: junto a esta app si está ahí y, si no, en el PATH.
fn bin(nombre: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| Some(e.parent()?.join(nombre)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(nombre))
}

/// Arranca `nombre` en segundo plano si no está en marcha (setsid: que no muera con la app).
fn arrancar(nombre: &str) -> io::Result<()> {
    if !en_marcha(nombre) {
        Command::new("setsid").arg("-f").arg(bin(nombre)).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
    }
    Ok(())
}

fn reloj_config() -> PathBuf {
    home().join(".config/moonlight-clock/config.toml")
}

fn modo_oscuro_path() -> PathBuf {
    home().join(".config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark")
}

/// Convierte un nombre en un id para la carpeta: "Atardecer Rojo" → "atardecer-rojo".
pub fn id_de(nombre: &str) -> String {
    let mut id = String::new();
    for c in nombre.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            'ñ' => 'n',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            id.push(c);
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }
    id.trim_matches('-').to_string()
}

pub fn listar() -> Vec<Fondo> {
    let mut fondos: Vec<Fondo> = fs::read_dir(raiz())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            let text = fs::read_to_string(dir.join("perfil.toml")).ok()?;
            let perfil = toml::from_str(&text).ok()?;
            Some(Fondo { id: e.file_name().to_string_lossy().into_owned(), dir, perfil })
        })
        .collect();
    fondos.sort_by(|a, b| a.perfil.nombre.to_lowercase().cmp(&b.perfil.nombre.to_lowercase()));
    fondos
}

/// Busca por id o por nombre (sin distinguir mayúsculas).
pub fn buscar(nombre: &str) -> Option<Fondo> {
    let id = id_de(nombre);
    listar().into_iter().find(|f| f.id == id || f.perfil.nombre.eq_ignore_ascii_case(nombre))
}

pub fn activo() -> Option<String> {
    fs::read_to_string(raiz().join("activo")).ok().map(|s| s.trim().to_string())
}

/// Copia una imagen al almacén. El perfil nuevo parte de la configuración que
/// hay puesta ahora mismo (visualizador, reloj y modo oscuro), con una paleta
/// sacada de la imagen.
pub fn anadir(imagen: &Path, nombre: &str) -> io::Result<Fondo> {
    let id = id_de(nombre);
    if id.is_empty() {
        return Err(io::Error::other("el nombre no puede estar vacío"));
    }
    let dir = raiz().join(&id);
    if dir.exists() {
        return Err(io::Error::other(format!("ya hay un fondo llamado «{id}»")));
    }
    let img = image::open(imagen).map_err(|e| io::Error::other(format!("no se pudo abrir la imagen: {e}")))?;

    fs::create_dir_all(&dir)?;
    let ext = imagen.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let archivo = format!("imagen.{ext}");
    fs::copy(imagen, dir.join(&archivo))?;
    miniatura(&img).save(dir.join("miniatura.png")).map_err(io::Error::other)?;

    let mut fondo = Fondo {
        id,
        dir,
        perfil: Perfil { nombre: nombre.to_string(), imagen: archivo, ..perfil_actual() },
    };
    if let Some(colores) = crate::paleta::desde_imagen(&fondo.miniatura(), 0) {
        fondo.perfil.tema.colores = colores;
    }
    fondo.guardar()?;
    Ok(fondo)
}

/// Cambia la imagen de un fondo conservando su configuración. El archivo
/// nuevo lleva otro nombre para que cosmic-bg note el cambio al aplicarlo.
pub fn cambiar_imagen(fondo: &mut Fondo, imagen: &Path) -> io::Result<()> {
    let img = image::open(imagen).map_err(|e| io::Error::other(format!("no se pudo abrir la imagen: {e}")))?;
    let ext = imagen.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let marca = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let archivo = format!("imagen-{marca}.{ext}");
    fs::copy(imagen, fondo.dir.join(&archivo))?;
    miniatura(&img).save(fondo.miniatura()).map_err(io::Error::other)?;
    let viejo = fondo.imagen();
    fondo.perfil.imagen = archivo;
    fondo.guardar()?;
    let _ = fs::remove_file(viejo);
    Ok(())
}

/// Un nombre que no esté cogido: "Atardecer", "Atardecer 2", "Atardecer 3"…
pub fn nombre_libre(base: &str) -> String {
    let base = base.trim();
    let base = if id_de(base).is_empty() { "Fondo" } else { base };
    (1..)
        .map(|n| if n == 1 { base.to_string() } else { format!("{base} {n}") })
        .find(|n| !raiz().join(id_de(n)).exists())
        .unwrap()
}

/// Nombre a partir del archivo: "atardecer_rojo-4k.png" → "Atardecer rojo 4k".
pub fn nombre_de_archivo(imagen: &Path) -> String {
    let base = imagen.file_stem().map(|s| s.to_string_lossy().replace(['_', '-'], " ")).unwrap_or_default();
    let base = base.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut letras = base.chars();
    match letras.next() {
        Some(c) => c.to_uppercase().chain(letras).collect(),
        None => String::new(),
    }
}

pub fn quitar(fondo: &Fondo) -> io::Result<()> {
    fs::remove_dir_all(&fondo.dir)?;
    if activo().as_deref() == Some(fondo.id.as_str()) {
        let _ = fs::remove_file(raiz().join("activo"));
    }
    Ok(())
}

/// Recorte 16:9 de 320×180 para la galería.
fn miniatura(img: &image::DynamicImage) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    let (cw, ch) = if w * 9 > h * 16 { (h * 16 / 9, h) } else { (w, w * 9 / 16) };
    img.crop_imm((w - cw) / 2, (h - ch) / 2, cw, ch)
        .resize_exact(320, 180, image::imageops::FilterType::Triangle)
}

/// Lo que hay configurado ahora mismo, para que un fondo nuevo empiece igual.
fn perfil_actual() -> Perfil {
    // Lo más fiel es el perfil del fondo puesto, si lo hay.
    if let Some(f) = activo().and_then(|id| listar().into_iter().find(|f| f.id == id)) {
        return f.perfil;
    }
    let mut p = Perfil::default();
    p.modo_oscuro = fs::read_to_string(modo_oscuro_path()).map_or(true, |s| s.trim() != "false");

    let vis = visualizador_config();
    if let Ok(v) = fs::read_to_string(vis).map(|t| toml::from_str(&t)) {
        p.visualizador = v.unwrap_or_default();
    }

    if let Ok(doc) = fs::read_to_string(reloj_config()).map(|t| t.parse::<toml_edit::DocumentMut>()) {
        if let Ok(doc) = doc {
            let r = &mut p.reloj;
            if let Some(v) = doc["window"]["anchor"].as_str() {
                r.posicion = v.into();
            }
            r.x = doc["window"]["x"].as_integer().unwrap_or(r.x);
            r.y = doc["window"]["y"].as_integer().unwrap_or(r.y);
            r.escala = doc["window"]["scale"].as_float().unwrap_or(r.escala);
            r.formato_hora = doc["clock"]["time_format"].as_str().map(String::from);
            r.tamano_hora = doc["appearance"]["time_font"]["size"].as_integer();
            if let Some(v) = doc["appearance"]["scheme"].as_str() {
                r.esquema = v.into();
            }
            for k in COLORES_RELOJ {
                if let Some(v) = doc["appearance"]["custom_colors"][k].as_str() {
                    r.colores.insert(k.into(), v.into());
                }
            }
        }
    }
    // Sin Moonlight Clock instalado, el reloj de texto viene encendido.
    p.reloj.mostrar = reloj_en_marcha() || !reloj_config().exists();
    p
}

/// Ruta del reloj; desde el lanzador de apps `~/.local/bin` puede no estar en el PATH.
fn reloj_bin() -> PathBuf {
    let local = home().join(".local/bin/moonlight-clock");
    if local.exists() { local } else { PathBuf::from("moonlight-clock") }
}

fn reloj_en_marcha() -> bool {
    Command::new(reloj_bin())
        .arg("status")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("\"running\": true"))
}

/// Pone todo lo del perfil. Devuelve los avisos de lo que no se pudo aplicar.
pub fn aplicar(fondo: &Fondo) -> Vec<String> {
    aplicar_con(fondo, true)
}

/// Como `aplicar`; sin `forzar_tema`, la estética solo se repinta si ha cambiado.
pub fn aplicar_con(fondo: &Fondo, forzar_tema: bool) -> Vec<String> {
    let mut avisos = Vec::new();
    let p = &fondo.perfil;

    if let Err(e) = poner_fondo(&fondo.imagen()) {
        avisos.push(format!("fondo de pantalla: {e}"));
    }

    let modo = modo_oscuro_path();
    let _ = fs::create_dir_all(modo.parent().unwrap());
    if let Err(e) = fs::write(&modo, if p.modo_oscuro { "true" } else { "false" }) {
        avisos.push(format!("modo oscuro: {e}"));
    }

    let vis = visualizador_config();
    let _ = fs::create_dir_all(vis.parent().unwrap());
    let text = toml::to_string_pretty(&p.visualizador).unwrap_or_default();
    if let Err(e) = fs::write(&vis, text) {
        avisos.push(format!("visualizador: {e}"));
    }
    if p.visualizador.mostrar {
        if let Err(e) = arrancar("ambient-visualizer") {
            avisos.push(format!("visualizador: {e}"));
        }
    }

    if let Err(e) = configurar_reloj(&p.reloj) {
        avisos.push(format!("reloj: {e}"));
    }

    if p.tema.aplicar {
        avisos.extend(crate::tema::aplicar(&p.tema, &p.nombre, forzar_tema));
    }

    if let Err(e) = fs::write(raiz().join("activo"), &fondo.id) {
        avisos.push(format!("guardar el activo: {e}"));
    }
    avisos
}

/// Cambia la imagen en la config de cosmic-bg (que se recarga sola).
fn poner_fondo(imagen: &Path) -> io::Result<()> {
    let dir = bg_dir();
    let mut archivos = vec![dir.join("all")];
    if let Ok(lista) = fs::read_to_string(dir.join("backgrounds")) {
        for salida in lista.split('"').skip(1).step_by(2) {
            archivos.push(dir.join(format!("output.{salida}")));
        }
    }
    let ruta = imagen.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let mut alguno = false;
    for archivo in archivos {
        let Ok(text) = fs::read_to_string(&archivo) else { continue };
        let Some(ini) = text.find("source: Path(\"").map(|i| i + "source: Path(\"".len()) else { continue };
        let Some(fin) = text[ini..].find("\")").map(|i| i + ini) else { continue };
        fs::write(&archivo, format!("{}{}{}", &text[..ini], ruta, &text[fin..]))?;
        alguno = true;
    }
    if alguno { Ok(()) } else { Err(io::Error::other("no se encontró la configuración de cosmic-bg")) }
}

/// Edita la config del Moonlight Clock respetando sus comentarios; él la
/// recarga solo en un segundo.
/// Enciende el reloj del estilo elegido y apaga el otro.
fn configurar_reloj(r: &Reloj) -> io::Result<()> {
    let texto = r.estilo == "texto";
    configurar_reloj_texto(r, texto && r.mostrar)?;
    configurar_moonlight(r, !texto && r.mostrar)
}

fn configurar_reloj_texto(r: &Reloj, mostrar: bool) -> io::Result<()> {
    #[derive(Serialize)]
    struct Config<'a> {
        mostrar: bool,
        posicion: &'a str,
        x: i64,
        y: i64,
        escala: f64,
        #[serde(flatten)]
        texto: &'a RelojTexto,
    }
    let mut texto = r.texto.clone();
    for c in [&mut texto.color_relleno, &mut texto.color_borde, &mut texto.color_sombra] {
        if !es_hex(c) {
            *c = "#ffffff".into();
        }
    }
    if !es_hex(&texto.color_fondo) {
        texto.color_fondo.clear();
    }
    let config = Config { mostrar, posicion: &r.posicion, x: r.x, y: r.y, escala: r.escala, texto: &texto };
    let path = home().join(".config/cosmic-ambient/clock.toml");
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, toml::to_string_pretty(&config).map_err(io::Error::other)?)?;
    if mostrar {
        arrancar("ambient-clock")?;
    }
    Ok(())
}

fn en_marcha(proceso: &str) -> bool {
    // El kernel guarda solo los 15 primeros caracteres del nombre del proceso.
    let nombre = &proceso[..proceso.len().min(15)];
    Command::new("pgrep").args(["-x", nombre]).output().is_ok_and(|o| o.status.success())
}

fn configurar_moonlight(r: &Reloj, mostrar: bool) -> io::Result<()> {
    let path = reloj_config();
    // El Moonlight Clock es opcional: sin él solo hay reloj de texto.
    if !path.exists() {
        return if mostrar { Err(io::Error::other("el Moonlight Clock no está instalado")) } else { Ok(()) };
    }
    let text = fs::read_to_string(&path)?;
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(io::Error::other)?;
    doc["window"]["anchor"] = toml_edit::value(r.posicion.as_str());
    doc["window"]["x"] = toml_edit::value(r.x);
    doc["window"]["y"] = toml_edit::value(r.y);
    doc["window"]["scale"] = toml_edit::value(r.escala.clamp(0.25, 4.0));
    doc["appearance"]["scheme"] = toml_edit::value(r.esquema.as_str());
    if let Some(formato) = &r.formato_hora {
        doc["clock"]["time_format"] = toml_edit::value(formato.as_str());
    }
    if let Some(tamano) = r.tamano_hora {
        doc["appearance"]["time_font"]["size"] = toml_edit::value(tamano.clamp(8, 180));
    }
    // Solo colores completos: mientras se escriben en la app pueden estar a medias.
    for (k, v) in r.colores.iter().filter(|(_, v)| es_hex(v)) {
        doc["appearance"]["custom_colors"][k.as_str()] = toml_edit::value(v.as_str());
    }
    fs::write(&path, doc.to_string())?;

    if mostrar != reloj_en_marcha() {
        // Sin tuberías: el reloj sigue vivo en segundo plano y output() esperaría.
        Command::new(reloj_bin())
            .arg(if mostrar { "start" } else { "stop" })
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
    }
    Ok(())
}

/// "#rrggbb" válido.
pub fn es_hex(s: &str) -> bool {
    let s = s.trim();
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}
