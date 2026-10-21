//! Visualizador de música para el fondo de escritorio de COSMIC.
//!
//! Crea una superficie layer-shell transparente en una esquina de la pantalla
//! (detrás de todas las ventanas) y dibuja, en columna:
//!   * portada, título, artista y progreso de la canción actual,
//!   * debajo, barras que se mueven con el audio que suena,
//! con colores sacados del fondo de escritorio. La esquina, los colores y si
//! se muestra se leen de `~/.config/cosmic-ambient/visualizer.toml`.

mod audio;
mod config;
mod palette;
mod player;
mod text;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
use tiny_skia::{
    Color, FillRule, FilterQuality, GradientStop, LinearGradient, Paint, Path, PathBuilder,
    Pattern, Pixmap, Point, SpreadMode, Transform,
};

// Diseño (en píxeles lógicos). Todo va en una columna cuadrada; las
// posiciones verticales son relativas a su borde superior.
const PANEL_X: f32 = 12.0;
/// Separación extra con el borde de la pantalla (margen de la capa).
const SCREEN_MARGIN: i32 = 36;
const PANEL_TOP: f32 = 20.0;
const PANEL: f32 = 360.0;
const PAD: f32 = 20.0;
const INNER: f32 = PANEL - 2.0 * PAD;
const ART: f32 = 150.0;
const TITLE_BASE: f32 = PAD + ART + 34.0;
const ARTIST_BASE: f32 = TITLE_BASE + 24.0;
const PROGRESS_Y: f32 = ARTIST_BASE + 16.0;
const TIMES_BASE: f32 = PROGRESS_Y + 20.0;
const N_BARS: usize = 32;
const BAR_GAP: f32 = 0.38; // fracción del hueco entre barras
const BARS_BASE: f32 = PANEL - PAD;
const BAR_MAX_H: f32 = 62.0;
const SURFACE_W: u32 = (2.0 * PANEL_X + PANEL) as u32;
const SURFACE_H: u32 = (PANEL_TOP + PANEL + 16.0) as u32;

const ACTIVE_FRAME: Duration = Duration::from_micros(16_667);
const IDLE_FRAME: Duration = Duration::from_millis(250);

fn main() {
    let layer = match std::env::args().nth(1).as_deref() {
        // La capa del fondo comparte sitio con cosmic-bg, que al cambiar de
        // imagen crea su superficie de nuevo y la pone encima: por eso se usa
        // la capa "bottom" (detrás de las ventanas igualmente).
        Some("--background") => Layer::Background,
        // Encima de todo, para probar el diseño con ventanas abiertas.
        Some("--overlay") => Layer::Overlay,
        Some("--palette") => {
            let path = palette::wallpaper_path();
            println!("{path:?}\n{:?}", path.as_ref().and_then(palette::from_image));
            return;
        }
        _ => Layer::Bottom,
    };

    let conn = Connection::connect_to_env().expect("no hay sesión Wayland");
    let (globals, event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new().unwrap();
    WaylandSource::new(conn.clone(), event_queue).insert(event_loop.handle()).unwrap();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor no disponible");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("layer-shell no disponible");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm no disponible");

    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, layer, Some("ambient-visualizer"), None);
    layer.set_size(SURFACE_W, SURFACE_H);
    layer.set_exclusive_zone(0);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    // Región de entrada vacía: los clics pasan al escritorio.
    let region = Region::new(&compositor).unwrap();
    layer.wl_surface().set_input_region(Some(region.wl_region()));

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new((SURFACE_W * SURFACE_H * 4) as usize, &shm).unwrap(),
        shm,
        layer,
        width: 0,
        height: 0,
        scale: 1,
        pixmap: None,
        fonts: text::Fonts::load(),
        ring: audio::spawn_capture(),
        spectrum: audio::Spectrum::new(N_BARS),
        now: player::spawn(ART as u32 * 2),
        drawn_version: u64::MAX,
        card_alpha: 0.0,
        wallpaper: palette::spawn(),
        colors: palette::DEFAULT,
        config: config::Config::default(),
        watcher: config::Watcher::new(),
        last_tick: Instant::now(),
        exit: false,
    };

    // Coloca la capa según la configuración y hace el primer commit.
    let initial = app.watcher.poll().unwrap_or_default();
    app.apply_config(initial);

    event_loop
        .handle()
        .insert_source(Timer::immediate(), |_, _, app: &mut App| {
            let active = app.tick();
            TimeoutAction::ToDuration(if active { ACTIVE_FRAME } else { IDLE_FRAME })
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
    width: u32,
    height: u32,
    scale: i32,
    pixmap: Option<Pixmap>,
    fonts: text::Fonts,
    ring: Arc<Mutex<audio::Ring>>,
    spectrum: audio::Spectrum,
    now: Arc<Mutex<player::NowPlaying>>,
    drawn_version: u64,
    card_alpha: f32,
    wallpaper: Arc<Mutex<palette::Palette>>,
    /// Paleta mostrada; se acerca poco a poco a la del fondo al cambiarlo.
    colors: palette::Palette,
    config: config::Config,
    watcher: config::Watcher,
    last_tick: Instant,
    exit: bool,
}

impl App {
    /// Avanza la animación y redibuja si hace falta. Devuelve si sigue habiendo
    /// movimiento (para decidir a qué ritmo volver a llamarla).
    fn tick(&mut self) -> bool {
        let dt = self.last_tick.elapsed().as_secs_f32().min(0.1);
        self.last_tick = Instant::now();
        if self.width == 0 {
            return false;
        }

        if let Some(cfg) = self.watcher.poll() {
            self.apply_config(cfg);
        }
        if !self.config.mostrar {
            if self.drawn_version != u64::MAX - 1 {
                self.drawn_version = u64::MAX - 1;
                self.draw(None);
            }
            return false;
        }

        self.spectrum.update(&self.ring, dt);

        let now = self.now.clone();
        let np = now.lock().unwrap();
        let playing = np.track.as_ref().is_some_and(|t| t.playing);

        let card_target = if np.track.is_some() { 1.0 } else { 0.0 };
        self.card_alpha = approach(self.card_alpha, card_target, dt * 4.0);
        let target = match self.config.manual() {
            Some((low, high)) => palette::Palette { low, high },
            None => *self.wallpaper.lock().unwrap(),
        };
        for (cur, t) in [(&mut self.colors.low, target.low), (&mut self.colors.high, target.high)] {
            for (c, t) in cur.iter_mut().zip(t) {
                *c = approach(*c, t, dt * 1.5);
            }
        }
        let animating = self.card_alpha != card_target || self.colors != target;

        let active = playing || animating || !self.spectrum.is_idle();
        if active || np.version != self.drawn_version {
            self.drawn_version = np.version;
            self.draw(Some(&np));
        }
        active
    }

    fn apply_config(&mut self, cfg: config::Config) {
        let vertical = if cfg.arriba() { Anchor::TOP } else { Anchor::BOTTOM };
        let (side, right, left) = if cfg.derecha() {
            (Anchor::RIGHT, SCREEN_MARGIN, 0)
        } else {
            (Anchor::LEFT, 0, SCREEN_MARGIN)
        };
        self.layer.set_anchor(vertical | side);
        self.layer.set_margin(0, right, 0, left);
        self.layer.commit();
        self.config = cfg;
        self.drawn_version = u64::MAX;
    }

    /// Dibuja un fotograma; con `None` deja la superficie vacía (oculto).
    fn draw(&mut self, np: Option<&player::NowPlaying>) {
        let s = self.scale as f32;
        let (w, h) = (self.width * self.scale as u32, self.height * self.scale as u32);
        if self.pixmap.as_ref().is_none_or(|p| p.width() != w || p.height() != h) {
            self.pixmap = Pixmap::new(w, h);
        }
        let Some(mut pix) = self.pixmap.take() else { return };
        pix.fill(Color::TRANSPARENT);

        if let Some(np) = np {
            self.draw_bars(&mut pix, s);
            if self.card_alpha > 0.0 {
                self.draw_card(&mut pix, np, s);
            }
        }

        // Copia al búfer compartido con el compositor.
        let stride = w as i32 * 4;
        let direct = self.shm.formats().contains(&wl_shm::Format::Abgr8888);
        let format = if direct { wl_shm::Format::Abgr8888 } else { wl_shm::Format::Argb8888 };
        let Ok((buffer, canvas)) = self.pool.create_buffer(w as i32, h as i32, stride, format) else {
            self.pixmap = Some(pix);
            return;
        };
        if direct {
            canvas.copy_from_slice(pix.data());
        } else {
            for (dst, src) in canvas.chunks_exact_mut(4).zip(pix.data().chunks_exact(4)) {
                dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
            }
        }

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w as i32, h as i32);
        buffer.attach_to(surface).unwrap();
        self.layer.commit();
        self.pixmap = Some(pix);
    }

    fn draw_bars(&self, pix: &mut Pixmap, s: f32) {
        let max_h = BAR_MAX_H * s;
        let base = (PANEL_TOP + BARS_BASE) * s;
        let slot = INNER * s / N_BARS as f32;
        let bar_w = slot * (1.0 - BAR_GAP);
        let radius = (bar_w / 2.0).min(6.0 * s);

        let palette::Palette { low, high } = self.colors;
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.shader = LinearGradient::new(
            Point::from_xy(0.0, base),
            Point::from_xy(0.0, base - max_h),
            vec![
                GradientStop::new(0.0, rgba(low, 0.55)),
                GradientStop::new(0.45, rgba(low, 0.95)),
                GradientStop::new(1.0, rgba(high, 0.95)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();

        let mut peak_paint = Paint::default();
        peak_paint.anti_alias = true;
        peak_paint.set_color(rgba(high, 0.9));

        for (i, (&v, &peak)) in self.spectrum.bars.iter().zip(&self.spectrum.peaks).enumerate() {
            let x = (PANEL_X + PAD) * s + i as f32 * slot + (slot - bar_w) / 2.0;
            if v > 0.002 {
                let bh = (v * max_h).max(radius * 2.0);
                if let Some(p) = round_rect(x, base - bh, bar_w, bh, radius) {
                    pix.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
                }
            }
            if peak > 0.02 {
                let cap = 3.0 * s;
                let y = base - peak * max_h - cap - 2.0 * s;
                if let Some(p) = round_rect(x, y, bar_w, cap, cap / 2.0) {
                    pix.fill_path(&p, &peak_paint, FillRule::Winding, Transform::identity(), None);
                }
            }
        }
    }

    fn draw_card(&self, pix: &mut Pixmap, np: &player::NowPlaying, s: f32) {
        let a = self.card_alpha;
        let left = (PANEL_X + PAD) * s;
        let top = PANEL_TOP * s;
        let inner = INNER * s;
        let cx = left + inner / 2.0;
        let art = ART * s;
        let (ax, ay) = (cx - art / 2.0, top + PAD * s);
        let id = Transform::identity();

        // Sombra + portada con esquinas redondeadas.
        let mut shadow = Paint::default();
        shadow.anti_alias = true;
        for k in 1..=4 {
            let k = k as f32 * 2.0 * s;
            shadow.set_color(Color::from_rgba(0.0, 0.0, 0.0, 0.08 * a).unwrap());
            if let Some(p) = round_rect(ax - k / 2.0, ay + k / 2.0, art + k, art + k, 14.0 * s + k) {
                pix.fill_path(&p, &shadow, FillRule::Winding, id, None);
            }
        }
        if let Some(cover) = round_rect(ax, ay, art, art, 12.0 * s) {
            let mut paint = Paint::default();
            paint.anti_alias = true;
            match &np.art {
                Some(img) => {
                    let k = art / img.height() as f32;
                    paint.shader = Pattern::new(
                        img.as_ref(),
                        SpreadMode::Pad,
                        FilterQuality::Bilinear,
                        a,
                        Transform::from_scale(k, k).post_translate(ax, ay),
                    );
                }
                None => paint.set_color(rgba(self.colors.low, 0.8 * a)),
            }
            pix.fill_path(&cover, &paint, FillRule::Winding, id, None);
        }

        let Some(track) = &np.track else { return };

        let centered = |pix: &mut Pixmap, font, size: f32, by: f32, t: &str, c: [f32; 4]| {
            let size = size * s;
            let t = self.fonts.fit(font, size, t, inner);
            let x = cx - self.fonts.measure(font, size, &t) / 2.0;
            // Sombra suave para que se lea sobre cualquier fondo.
            self.fonts.draw(pix, font, size, x + 1.0 * s, by + 1.5 * s, &t, [0.0, 0.0, 0.0, 0.4 * c[3]]);
            self.fonts.draw(pix, font, size, x, by, &t, c);
        };

        centered(pix, &self.fonts.bold, 24.0, top + TITLE_BASE * s, &track.title, [1.0, 1.0, 1.0, a]);
        let mut sub = track.artist.clone();
        if !track.album.is_empty() && track.album != track.title {
            sub = if sub.is_empty() { track.album.clone() } else { format!("{sub} · {}", track.album) };
        }
        centered(pix, &self.fonts.regular, 16.0, top + ARTIST_BASE * s, &sub, [1.0, 1.0, 1.0, 0.8 * a]);

        // Barra de progreso y tiempos.
        if track.length_us > 0 {
            let by = top + PROGRESS_Y * s;
            let frac = (np.position_now() as f32 / track.length_us as f32).clamp(0.0, 1.0);
            let mut paint = Paint::default();
            paint.anti_alias = true;
            paint.set_color(Color::from_rgba(1.0, 1.0, 1.0, 0.2 * a).unwrap());
            if let Some(p) = round_rect(left, by, inner, 4.0 * s, 2.0 * s) {
                pix.fill_path(&p, &paint, FillRule::Winding, id, None);
            }
            paint.shader = LinearGradient::new(
                Point::from_xy(left, by),
                Point::from_xy(left + inner, by),
                vec![
                    GradientStop::new(0.0, rgba(self.colors.low, a)),
                    GradientStop::new(1.0, rgba(self.colors.high, a)),
                ],
                SpreadMode::Pad,
                id,
            )
            .unwrap();
            if let Some(p) = round_rect(left, by, (inner * frac).max(4.0 * s), 4.0 * s, 2.0 * s) {
                pix.fill_path(&p, &paint, FillRule::Winding, id, None);
            }

            let size = 13.0 * s;
            let tb = top + TIMES_BASE * s;
            let dim = [1.0, 1.0, 1.0, 0.65 * a];
            let font = &self.fonts.regular;
            self.fonts.draw(pix, font, size, left, tb, &mmss(np.position_now()), dim);
            let total = mmss(track.length_us);
            let tw = self.fonts.measure(font, size, &total);
            self.fonts.draw(pix, font, size, left + inner - tw, tb, &total, dim);
            if !track.playing {
                let pw = self.fonts.measure(font, size, "En pausa");
                self.fonts.draw(pix, font, size, cx - pw / 2.0, tb, "En pausa", dim);
            }
        }
    }
}

fn rgba([r, g, b]: [f32; 3], a: f32) -> Color {
    Color::from_rgba(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a.clamp(0.0, 1.0)).unwrap()
}

fn approach(cur: f32, target: f32, step: f32) -> f32 {
    if (target - cur).abs() <= step { target } else { cur + step.copysign(target - cur) }
}

fn mmss(us: u64) -> String {
    let secs = us / 1_000_000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, factor: i32) {
        self.scale = factor.max(1);
        surface.set_buffer_scale(self.scale);
        self.drawn_version = u64::MAX;
    }
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
        self.width = if configure.new_size.0 == 0 { SURFACE_W } else { configure.new_size.0 };
        self.height = if configure.new_size.1 == 0 { SURFACE_H } else { configure.new_size.1 };
        self.drawn_version = u64::MAX;
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
