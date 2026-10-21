//! Reloj de solo texto para el escritorio de COSMIC: hora y fecha con relleno,
//! borde y sombra de colores. Sin fondo detrás, o sobre una tarjeta de color
//! redondeada si se pone `color_fondo`.
//!
//! Lee `~/.config/cosmic-ambient/clock.toml` (lo escribe `cosmic-ambient`) y lo
//! relee solo cuando cambia. Con `mostrar = false` se queda invisible.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use serde::Deserialize;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            timer::{TimeoutAction, Timer},
            EventLoop,
        },
        calloop_wayland_source::WaylandSource,
        client::{
            globals::registry_queue_init,
            protocol::{wl_output, wl_shm, wl_surface},
            Connection, QueueHandle,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

// Tamaños base (se multiplican por `escala`).
const HORA_PX: f32 = 150.0;
const FECHA_PX: f32 = 64.0;
const SEPARACION: f32 = 6.0;
// Tarjeta: margen interior y radio de las esquinas.
const TARJETA_PAD_X: f32 = 34.0;
const TARJETA_PAD_Y: f32 = 20.0;
const TARJETA_RADIO: f32 = 28.0;

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Config {
    mostrar: bool,
    /// Igual que el Moonlight Clock: "top left", "top right", "center"...
    posicion: String,
    x: i32,
    y: i32,
    escala: f32,
    fuente: String,
    color_relleno: String,
    color_borde: String,
    color_sombra: String,
    /// Tarjeta detrás del texto ("" = sin tarjeta). Con tarjeta, la sombra es
    /// la de la tarjeta y el texto va centrado en ella.
    color_fondo: String,
    /// Grosor del borde y desplazamiento de la sombra, en píxeles a escala 1.
    grosor_borde: f32,
    sombra: f32,
    /// Formato de chrono, p. ej. "%H:%M" o "%H:%M:%S".
    formato_hora: String,
    formato_fecha: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mostrar: false,
            posicion: "top left".into(),
            x: 40,
            y: 40,
            escala: 1.0,
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

impl Config {
    fn derecha(&self) -> bool {
        self.posicion.ends_with("right")
    }

    fn centrado(&self) -> bool {
        !self.posicion.ends_with("left") && !self.derecha()
    }

    fn tarjeta(&self) -> bool {
        self.color_fondo.trim_start_matches('#').len() == 6
    }
}

fn config_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/cosmic-ambient/clock.toml")
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn leer_config() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

fn cargar_fuente(patron: &str) -> FontVec {
    let fc = |p: &str| -> Option<FontVec> {
        let out = Command::new("fc-match").args(["-f", "%{file}|%{index}", p]).output().ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        let (file, index) = text.split_once('|')?;
        FontVec::try_from_vec_and_index(std::fs::read(file).ok()?, index.parse().unwrap_or(0)).ok()
    };
    fc(patron).or_else(|| fc("sans:bold")).expect("no se encontró ninguna fuente")
}

fn hex(s: &str) -> [f32; 3] {
    let s = s.trim().trim_start_matches('#');
    let c = |i: usize| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(255) as f32 / 255.0;
    [c(0), c(2), c(4)]
}

/// Ancho de `text` contando cada cifra como la más ancha, para que el reloj
/// no cambie de tamaño según la hora.
fn ancho_maximo(font: &FontVec, size: f32, text: &str) -> f32 {
    let sf = font.as_scaled(PxScale::from(size));
    let cifra = ('0'..='9').map(|c| sf.h_advance(font.glyph_id(c))).fold(0.0, f32::max);
    text.chars()
        .map(|c| if c.is_ascii_digit() { cifra } else { sf.h_advance(font.glyph_id(c)) })
        .sum()
}

/// Lienzo de cobertura (0..1) donde se escribe el texto.
struct Mascara {
    w: usize,
    h: usize,
    v: Vec<f32>,
}

impl Mascara {
    fn new(w: usize, h: usize) -> Self {
        Mascara { w, h, v: vec![0.0; w * h] }
    }

    fn texto(&mut self, font: &FontVec, size: f32, x: f32, baseline: f32, text: &str) -> f32 {
        let scale = PxScale::from(size);
        let sf = font.as_scaled(scale);
        let mut caret = x;
        let mut prev = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                caret += sf.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scale, (caret, baseline));
            if let Some(outline) = font.outline_glyph(glyph) {
                let b = outline.px_bounds();
                outline.draw(|gx, gy, cov| {
                    let px = b.min.x as i32 + gx as i32;
                    let py = b.min.y as i32 + gy as i32;
                    if px >= 0 && py >= 0 && (px as usize) < self.w && (py as usize) < self.h {
                        let i = py as usize * self.w + px as usize;
                        self.v[i] = self.v[i].max(cov.min(1.0));
                    }
                });
            }
            caret += sf.h_advance(id);
            prev = Some(id);
        }
        caret - x
    }

    /// Rectángulo de esquinas redondeadas de (x0, y0) a (x1, y1).
    fn redondeado(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) {
        let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        for y in 0..self.h {
            for x in 0..self.w {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                // Distancia al rectángulo interior encogido `r`, menos `r`.
                let dx = (x0 + r - px).max(px - (x1 - r)).max(0.0);
                let dy = (y0 + r - py).max(py - (y1 - r)).max(0.0);
                let fuera = (dx * dx + dy * dy).sqrt() - r;
                let borde = (x0 - px).max(px - x1).max(y0 - py).max(py - y1);
                let d = if dx > 0.0 && dy > 0.0 { fuera } else { borde };
                self.v[y * self.w + x] = (0.5 - d).clamp(0.0, 1.0);
            }
        }
    }

    /// Engorda la máscara `r` píxeles en redondo (para el borde).
    fn dilatar(&self, r: f32) -> Mascara {
        let ri = r.ceil() as i32;
        let offsets: Vec<(i32, i32, f32)> = (-ri..=ri)
            .flat_map(|dy| (-ri..=ri).map(move |dx| (dx, dy)))
            .filter_map(|(dx, dy)| {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                // Borde suave en el último píxel.
                let peso = (r + 0.5 - d).clamp(0.0, 1.0);
                (peso > 0.0).then_some((dx, dy, peso))
            })
            .collect();
        let mut out = Mascara::new(self.w, self.h);
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                let mut m = 0.0f32;
                for &(dx, dy, peso) in &offsets {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx >= 0 && sy >= 0 && (sx as usize) < self.w && (sy as usize) < self.h {
                        m = m.max(self.v[sy as usize * self.w + sx as usize] * peso);
                        if m >= 1.0 {
                            break;
                        }
                    }
                }
                out.v[y as usize * self.w + x as usize] = m;
            }
        }
        out
    }
}

/// Pinta `color` con la cobertura de `m` (desplazada) sobre un lienzo RGBA
/// premultiplicado.
fn pintar(lienzo: &mut [[f32; 4]], m: &Mascara, color: [f32; 3], dx: i32, dy: i32) {
    for y in 0..m.h as i32 {
        for x in 0..m.w as i32 {
            let (sx, sy) = (x - dx, y - dy);
            if sx < 0 || sy < 0 || sx as usize >= m.w || sy as usize >= m.h {
                continue;
            }
            let a = m.v[sy as usize * m.w + sx as usize];
            if a <= 0.0 {
                continue;
            }
            let p = &mut lienzo[y as usize * m.w + x as usize];
            let inv = 1.0 - a;
            for c in 0..3 {
                p[c] = color[c] * a + p[c] * inv;
            }
            p[3] = a + p[3] * inv;
        }
    }
}

fn main() {
    let conn = Connection::connect_to_env().expect("no hay sesión Wayland");
    let (globals, event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new().unwrap();
    WaylandSource::new(conn.clone(), event_queue).insert(event_loop.handle()).unwrap();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor no disponible");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("layer-shell no disponible");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm no disponible");

    let surface = compositor.create_surface(&qh);
    // `--overlay`: encima de las ventanas, para probar.
    let capa = if std::env::args().any(|a| a == "--overlay") { Layer::Overlay } else { Layer::Bottom };
    let layer = layer_shell.create_layer_surface(&qh, surface, capa, Some("ambient-clock"), None);
    layer.set_exclusive_zone(0);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    // Región de entrada vacía: los clics pasan al escritorio.
    let region = Region::new(&compositor).unwrap();
    layer.wl_surface().set_input_region(Some(region.wl_region()));

    let config = leer_config();
    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(512 * 256 * 4, &shm).unwrap(),
        shm,
        layer,
        fuente: cargar_fuente(&config.fuente),
        config_mtime: mtime(&config_path()),
        config,
        size: (1, 1),
        configured: false,
        dibujado: String::new(),
        exit: false,
    };
    app.colocar();

    event_loop
        .handle()
        .insert_source(Timer::immediate(), |_, _, app: &mut App| {
            app.tick();
            TimeoutAction::ToDuration(Duration::from_millis(500))
        })
        .unwrap();

    while !app.exit {
        event_loop.dispatch(None, &mut app).unwrap();
    }
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    fuente: FontVec,
    config: Config,
    config_mtime: Option<SystemTime>,
    size: (u32, u32),
    configured: bool,
    /// Texto del último fotograma, para no redibujar si no ha cambiado.
    dibujado: String,
    exit: bool,
}

impl App {
    fn medidas(&self) -> (f32, f32, f32, f32) {
        let e = self.config.escala.clamp(0.2, 4.0);
        let c = &self.config;
        (HORA_PX * e, FECHA_PX * e, c.grosor_borde.clamp(0.0, 20.0) * e, c.sombra.clamp(0.0, 30.0) * e)
    }

    /// Tamaño y posición de la capa según la configuración.
    fn colocar(&mut self) {
        let (hora_px, fecha_px, borde, sombra) = self.medidas();
        let ahora = chrono::Local::now();
        let hora = ahora.format(&self.config.formato_hora).to_string();
        let fecha = ahora.format(&self.config.formato_fecha).to_string();
        let pad = borde + sombra + 4.0;
        let w = ancho_maximo(&self.fuente, hora_px, &hora).max(ancho_maximo(&self.fuente, fecha_px, &fecha));
        let alto = |px: f32| {
            let sf = self.fuente.as_scaled(PxScale::from(px));
            sf.ascent() - sf.descent()
        };
        let h = alto(hora_px) + SEPARACION + alto(fecha_px);
        let size = if self.config.tarjeta() {
            let e = self.config.escala.clamp(0.2, 4.0);
            let (px, py) = (TARJETA_PAD_X * e + borde, TARJETA_PAD_Y * e + borde);
            ((w + 2.0 * px + sombra + 4.0).ceil() as u32, (h + 2.0 * py + sombra + 4.0).ceil() as u32)
        } else {
            ((w + 2.0 * pad).ceil() as u32, (h + 2.0 * pad).ceil() as u32)
        };

        let p = self.config.posicion.as_str();
        let mut anchor = Anchor::empty();
        if p.starts_with("top") {
            anchor |= Anchor::TOP;
        }
        if p.starts_with("bottom") {
            anchor |= Anchor::BOTTOM;
        }
        if p.ends_with("left") {
            anchor |= Anchor::LEFT;
        }
        if p.ends_with("right") {
            anchor |= Anchor::RIGHT;
        }
        let (x, y) = (self.config.x, self.config.y);
        self.layer.set_anchor(anchor);
        self.layer.set_margin(y, x, y, x);
        self.layer.set_size(size.0, size.1);
        self.layer.commit();
        self.size = size;
        self.dibujado.clear();
    }

    fn tick(&mut self) {
        let m = mtime(&config_path());
        if m != self.config_mtime {
            self.config_mtime = m;
            let nueva = leer_config();
            if nueva.fuente != self.config.fuente {
                self.fuente = cargar_fuente(&nueva.fuente);
            }
            self.config = nueva;
            self.colocar();
        }
        if !self.configured {
            return;
        }
        let ahora = chrono::Local::now();
        let hora = ahora.format(&self.config.formato_hora).to_string();
        let fecha = ahora.format(&self.config.formato_fecha).to_string();
        let clave = format!("{}|{hora}|{fecha}", self.config.mostrar);
        if clave != self.dibujado {
            self.dibujado = clave;
            self.dibujar(&hora, &fecha);
        }
    }

    fn dibujar(&mut self, hora: &str, fecha: &str) {
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        let mut lienzo = vec![[0.0f32; 4]; w * h];

        if self.config.mostrar {
            let (hora_px, fecha_px, borde, sombra) = self.medidas();
            let pad = borde + sombra + 4.0;
            let mut mascara = Mascara::new(w, h);
            let sf_h = self.fuente.as_scaled(PxScale::from(hora_px));
            let sf_f = self.fuente.as_scaled(PxScale::from(fecha_px));
            let tarjeta = self.config.tarjeta();
            // La tarjeta ocupa todo menos el hueco de su sombra.
            let (tx1, ty1) = (w as f32 - 2.0 - sombra, h as f32 - 2.0 - sombra);
            let alto_bloque = sf_h.ascent() - sf_h.descent() + SEPARACION + sf_f.ascent() - sf_f.descent();
            let base_hora = if tarjeta { 2.0 + (ty1 - 2.0 - alto_bloque) / 2.0 } else { pad } + sf_h.ascent();
            let base_fecha = base_hora - sf_h.descent() + SEPARACION + sf_f.ascent();

            // Alineado hacia el lado del que cuelga el reloj (centrado en la tarjeta).
            let x_de = |ancho: f32| {
                if tarjeta {
                    (2.0 + tx1 - ancho) / 2.0
                } else if self.config.derecha() {
                    w as f32 - pad - sombra - ancho
                } else if self.config.centrado() {
                    (w as f32 - ancho) / 2.0
                } else {
                    pad - sombra
                }
            };
            let ancho_hora = Mascara::new(0, 0).texto(&self.fuente, hora_px, 0.0, 0.0, hora);
            let ancho_fecha = Mascara::new(0, 0).texto(&self.fuente, fecha_px, 0.0, 0.0, fecha);
            mascara.texto(&self.fuente, hora_px, x_de(ancho_hora), base_hora, hora);
            mascara.texto(&self.fuente, fecha_px, x_de(ancho_fecha), base_fecha, fecha);

            // Sin borde, la sombra sigue la silueta de las letras.
            let gorda = (borde > 0.0).then(|| mascara.dilatar(borde));
            let silueta = gorda.as_ref().unwrap_or(&mascara);
            let s = sombra.round() as i32;
            if tarjeta {
                let mut fondo = Mascara::new(w, h);
                fondo.redondeado(2.0, 2.0, tx1, ty1, TARJETA_RADIO * self.config.escala.clamp(0.2, 4.0));
                if s > 0 {
                    pintar(&mut lienzo, &fondo, hex(&self.config.color_sombra), s, s);
                }
                pintar(&mut lienzo, &fondo, hex(&self.config.color_fondo), 0, 0);
            } else if s > 0 {
                pintar(&mut lienzo, silueta, hex(&self.config.color_sombra), s, s);
            }
            if let Some(gorda) = &gorda {
                pintar(&mut lienzo, gorda, hex(&self.config.color_borde), 0, 0);
            }
            pintar(&mut lienzo, &mascara, hex(&self.config.color_relleno), 0, 0);
        }

        let stride = w as i32 * 4;
        let Ok((buffer, canvas)) =
            self.pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };
        // Argb8888 en memoria (little endian) es B, G, R, A.
        for (dst, p) in canvas.chunks_exact_mut(4).zip(&lienzo) {
            let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            dst.copy_from_slice(&[b(p[2]), b(p[1]), b(p[0]), b(p[3])]);
        }
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w as i32, h as i32);
        buffer.attach_to(surface).unwrap();
        self.layer.commit();
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        if configure.new_size.0 > 0 && configure.new_size.1 > 0 {
            self.size = configure.new_size;
        }
        self.configured = true;
        self.dibujado.clear();
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_layer!(App);
delegate_registry!(App);
