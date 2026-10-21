//! Ventana: galería de fondos a la izquierda y configuración del elegido a la
//! derecha. Los cambios se guardan al momento en su perfil.toml y, si ese
//! fondo es el que está puesto, se aplican también al momento.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use cosmic::app::{Settings, Task};
use cosmic::iced::widget::container;
use cosmic::iced::{Alignment, Background, Border, Color, Length, Size, Subscription};
use cosmic::prelude::*;
use cosmic::widget::{self, settings};
use cosmic::{Core, executor, iced};

use crate::store::{self, Fondo};

const ESQUINAS_TXT: [&str; 4] =
    ["Abajo a la izquierda", "Abajo a la derecha", "Arriba a la izquierda", "Arriba a la derecha"];
const COLORES_VIS_TXT: [&str; 2] = ["Sacados del fondo", "Elegidos a mano"];
const POSICIONES_TXT: [&str; 9] = [
    "Arriba izquierda", "Arriba centro", "Arriba derecha", "Centro izquierda", "Centro",
    "Centro derecha", "Abajo izquierda", "Abajo centro", "Abajo derecha",
];
const ESQUEMAS_TXT: [&str; 10] = [
    "Azul", "Rosa", "Verde", "Amarillo", "Rojo", "Verde claro", "Morado", "Azul oscuro", "Gris",
    "Personalizado",
];
const ESTILOS_RELOJ: [&str; 2] = ["moonlight", "texto"];
const ESTILOS_RELOJ_TXT: [&str; 2] = ["Moonlight (con forma)", "Solo texto"];
const COLORES_TEXTO_TXT: [&str; 4] = ["Relleno", "Borde", "Sombra", "Tarjeta (vacío = sin)"];
const COLORES_TEMA_TXT: [&str; 4] = ["Acento", "Acento secundario", "Fondo de ventanas", "Texto"];
const COLORES_RELOJ_TXT: [&str; 7] =
    ["Fondo de arriba", "Fondo de abajo", "Hora", "Fecha", "Texto de abajo", "Sombra", "Números destacados"];

pub fn run() {
    let settings = Settings::default().size(Size::new(1180.0, 900.0));
    if let Err(e) = cosmic::app::run::<App>(settings, ()) {
        eprintln!("{e}");
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    Seleccionar(usize),
    Aplicar,
    Recargar,
    ModoOscuro(bool),
    VisMostrar(bool),
    VisEsquina(usize),
    VisColores(usize),
    VisBase(String),
    VisPunta(String),
    RelojMostrar(bool),
    RelojPosicion(usize),
    RelojEstilo(usize),
    RelojFuente(String),
    /// Relleno, borde o sombra del reloj de texto.
    RelojTextoColor(usize, String),
    /// Mientras se arrastra el control de tamaño (aún sin guardar).
    RelojEscala(f32),
    /// Al soltarlo: se guarda y se aplica.
    RelojEscalaSoltar,
    RelojEsquema(usize),
    RelojColor(usize, String),
    TemaAplicar(bool),
    /// Uno de `tema::PRINCIPALES`.
    TemaColor(usize, String),
}

pub struct App {
    core: Core,
    fondos: Vec<Fondo>,
    /// Miniatura de cada fondo y la fecha del archivo, para recargarla si cambia.
    miniaturas: HashMap<String, (Option<SystemTime>, widget::image::Handle)>,
    /// Fondos y fechas de sus archivos; si cambia, algo se tocó desde fuera.
    firma: Vec<(String, Option<SystemTime>, Option<SystemTime>)>,
    sel: usize,
    activo: Option<String>,
    estado: String,
}

impl App {
    /// Relee el almacén (por si se han añadido o quitado fondos desde la
    /// terminal) manteniendo el fondo seleccionado.
    fn recargar(&mut self) {
        let sel_id = self.fondos.get(self.sel).map(|f| f.id.clone());
        self.fondos = store::listar();
        self.activo = store::activo();
        for f in &self.fondos {
            let fecha = fecha(&f.miniatura());
            if self.miniaturas.get(&f.id).is_none_or(|(m, _)| *m != fecha) {
                if let Ok(img) = image::open(f.miniatura()) {
                    let img = img.to_rgba8();
                    let handle = widget::image::Handle::from_rgba(img.width(), img.height(), img.into_raw());
                    self.miniaturas.insert(f.id.clone(), (fecha, handle));
                }
            }
        }
        let preferido = sel_id.or_else(|| self.activo.clone());
        self.sel = preferido
            .and_then(|id| self.fondos.iter().position(|f| f.id == id))
            .unwrap_or(0);
    }

    fn es_activo(&self, f: &Fondo) -> bool {
        self.activo.as_deref() == Some(f.id.as_str())
    }

    fn aplicar_sel(&mut self) {
        self.aplicar_sel_con(true);
    }

    fn aplicar_sel_con(&mut self, forzar_tema: bool) {
        let Some(fondo) = self.fondos.get(self.sel) else { return };
        let avisos = store::aplicar_con(fondo, forzar_tema);
        self.activo = Some(fondo.id.clone());
        self.estado = if avisos.is_empty() {
            format!("«{}» puesto.", fondo.perfil.nombre)
        } else {
            format!("Puesto, con avisos: {}", avisos.join("; "))
        };
    }

    /// Cambia el perfil seleccionado, lo guarda y, si está puesto, lo aplica.
    fn editar(&mut self, cambio: impl FnOnce(&mut store::Perfil)) {
        let Some(fondo) = self.fondos.get_mut(self.sel) else { return };
        cambio(&mut fondo.perfil);
        if let Err(e) = fondo.guardar() {
            self.estado = format!("No se pudo guardar: {e}");
            return;
        }
        if self.activo.as_deref() == Some(fondo.id.as_str()) {
            self.aplicar_sel_con(false);
        } else {
            self.estado = "Guardado. Pulsa «Poner este fondo» para verlo.".into();
        }
    }

    fn galeria(&self) -> Element<'_, Message> {
        let tarjetas: Vec<Element<'_, Message>> = self
            .fondos
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let imagen: Element<'_, Message> = match self.miniaturas.get(&f.id) {
                    Some((_, h)) => widget::image(h.clone()).width(240).height(135).into(),
                    None => widget::container(widget::text::caption("sin miniatura"))
                        .width(240)
                        .height(135)
                        .center(Length::Fill)
                        .into(),
                };
                let nombre = if self.es_activo(f) {
                    format!("{} · puesto", f.perfil.nombre)
                } else {
                    f.perfil.nombre.clone()
                };
                widget::button::custom(
                    widget::column::with_capacity(4)
                        .push(imagen)
                        .push(widget::text::body(nombre))
                        .spacing(6)
                        .align_x(Alignment::Center)
                        .width(Length::Fixed(240.0)),
                )
                .class(cosmic::theme::Button::Image)
                .selected(i == self.sel)
                .padding(6)
                .width(Length::Fixed(252.0))
                .on_press(Message::Seleccionar(i))
                .into()
            })
            .collect();

        // Filas de dos miniaturas (la flex_row de libcosmic estiraba las tarjetas).
        let mut filas = widget::column::with_capacity(tarjetas.len() / 2 + 1).spacing(12);
        let mut tarjetas = tarjetas.into_iter().peekable();
        while tarjetas.peek().is_some() {
            filas = filas.push(widget::row::with_children(tarjetas.by_ref().take(2).collect::<Vec<_>>()).spacing(12));
        }
        widget::scrollable(filas).width(Length::Fill).height(Length::Fill).into()
    }

    fn panel(&self) -> Element<'_, Message> {
        let f = &self.fondos[self.sel];
        let p = &f.perfil;

        let boton: Element<'_, Message> = if self.es_activo(f) {
            widget::button::standard("Es el fondo puesto").into()
        } else {
            widget::button::suggested("Poner este fondo").on_press(Message::Aplicar).into()
        };

        let escritorio = settings::section()
            .title("Escritorio")
            .add(settings::item("Modo oscuro", widget::toggler(p.modo_oscuro).on_toggle(Message::ModoOscuro)));

        let v = &p.visualizador;
        let manual = v.colores == "manual";
        let mut vis = settings::section()
            .title("Visualizador de música")
            .add(settings::item("Mostrar", widget::toggler(v.mostrar).on_toggle(Message::VisMostrar)))
            .add(settings::item(
                "Esquina",
                widget::dropdown(&ESQUINAS_TXT[..], indice(&store::ESQUINAS, &v.esquina), Message::VisEsquina),
            ))
            .add(settings::item(
                "Colores",
                widget::dropdown(&COLORES_VIS_TXT[..], Some(manual as usize), Message::VisColores),
            ));
        if manual {
            vis = vis
                .add(settings::item("Color de la base", color_input(&v.color_base, Message::VisBase)))
                .add(settings::item("Color de la punta", color_input(&v.color_punta, Message::VisPunta)));
        }

        let r = &p.reloj;
        let mut reloj = settings::section()
            .title("Reloj")
            .add(settings::item("Mostrar", widget::toggler(r.mostrar).on_toggle(Message::RelojMostrar)))
            .add(settings::item(
                "Estilo",
                widget::dropdown(&ESTILOS_RELOJ_TXT[..], indice(&ESTILOS_RELOJ, &r.estilo), Message::RelojEstilo),
            ))
            .add(settings::item(
                "Posición",
                widget::dropdown(&POSICIONES_TXT[..], indice(&store::POSICIONES_RELOJ, &r.posicion), Message::RelojPosicion),
            ))
            .add(settings::item(
                "Tamaño",
                widget::row::with_capacity(2)
                    .push(
                        widget::slider(0.25..=2.0, r.escala as f32, Message::RelojEscala)
                            .step(0.05_f32)
                            .on_release(Message::RelojEscalaSoltar)
                            .width(160),
                    )
                    .push(widget::text::body(format!("{:.0} %", r.escala * 100.0)).width(48))
                    .spacing(12)
                    .align_y(Alignment::Center),
            ));
        if r.estilo == "texto" {
            let t = &r.texto;
            reloj = reloj.add(settings::item(
                "Tipografía",
                widget::text_input("Playfair Display", t.fuente.as_str()).on_input(Message::RelojFuente).width(160),
            ));
            for (i, valor) in [&t.color_relleno, &t.color_borde, &t.color_sombra, &t.color_fondo].into_iter().enumerate() {
                reloj = reloj.add(settings::item(
                    COLORES_TEXTO_TXT[i],
                    color_input(valor, move |s| Message::RelojTextoColor(i, s)),
                ));
            }
        } else {
            reloj = reloj.add(settings::item(
                "Colores",
                widget::dropdown(&ESQUEMAS_TXT[..], indice(&store::ESQUEMAS_RELOJ, &r.esquema), Message::RelojEsquema),
            ));
        }
        if r.estilo != "texto" && r.esquema == "custom" {
            for (i, clave) in store::COLORES_RELOJ.iter().enumerate() {
                let valor = r.colores.get(*clave).map_or("", String::as_str);
                reloj = reloj.add(settings::item(
                    COLORES_RELOJ_TXT[i],
                    color_input(valor, move |s| Message::RelojColor(i, s)),
                ));
            }
        }

        let mut col = widget::column::with_capacity(4)
            .push(widget::text::title3(p.nombre.as_str()))
            .push(boton)
            .spacing(16);
        if !self.estado.is_empty() {
            col = col.push(widget::text::caption(self.estado.as_str()));
        }
        let t = &p.tema;
        let mut tema = settings::section().title("Estética del sistema").add(settings::item(
            "Teñir el escritorio y las apps",
            widget::toggler(t.aplicar).on_toggle(Message::TemaAplicar),
        ));
        if t.aplicar {
            for (i, n) in crate::tema::PRINCIPALES.iter().enumerate() {
                let valor = t.colores.get(*n).map_or("", String::as_str);
                tema = tema.add(settings::item(
                    COLORES_TEMA_TXT[i],
                    color_input(valor, move |s| Message::TemaColor(i, s)),
                ));
            }
            tema = tema.add(widget::text::caption(
                "Tiñe el tema de COSMIC (y con él la terminal y las apps GTK). zsh, starship, \
                 bat, btop y VS Code se activan en ~/.config/cosmic-ambient/integraciones.toml. \
                 La paleta completa (26 colores) está en perfil.toml.",
            ));
        }

        col.push(escritorio).push(tema).push(vis).push(reloj).into()
    }
}

fn fecha(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn firma() -> Vec<(String, Option<SystemTime>, Option<SystemTime>)> {
    store::listar()
        .into_iter()
        .map(|f| (f.id.clone(), fecha(&f.miniatura()), fecha(&f.dir.join("perfil.toml"))))
        .collect()
}

fn indice(opciones: &[&str], valor: &str) -> Option<usize> {
    opciones.iter().position(|o| *o == valor)
}

/// Campo "#rrggbb" con una muestra del color al lado.
fn color_input<'a>(valor: &'a str, on_input: impl Fn(String) -> Message + 'a) -> Element<'a, Message> {
    let color = store::es_hex(valor).then(|| {
        let c = |i: usize| u8::from_str_radix(&valor.trim()[i..i + 2], 16).unwrap_or(0);
        Color::from_rgb8(c(1), c(3), c(5))
    });
    let muestra = widget::container(widget::text(""))
        .width(22)
        .height(22)
        .class(cosmic::theme::Container::custom(move |_| container::Style {
            background: color.map(Background::Color),
            border: Border { radius: 6.0.into(), width: 1.0, color: Color::from_rgba(0.5, 0.5, 0.5, 0.6) },
            ..Default::default()
        }));
    widget::row::with_capacity(4)
        .push(muestra)
        .push(widget::text_input("#rrggbb", valor).on_input(on_input).width(110))
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

impl cosmic::Application for App {
    type Executor = executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "io.github.sarajimenezlara.Ambient";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _: ()) -> (Self, Task<Message>) {
        let mut app = App {
            core,
            fondos: Vec::new(),
            miniaturas: HashMap::new(),
            firma: firma(),
            sel: 0,
            activo: None,
            estado: String::new(),
        };
        app.recargar();
        app.set_header_title("Ambient".into());
        let titulo = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Ambient".into(), id),
            None => Task::none(),
        };
        (app, titulo)
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_secs(2)).map(|_| Message::Recargar)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Seleccionar(i) => {
                self.sel = i;
                self.estado.clear();
            }
            Message::Aplicar => self.aplicar_sel(),
            Message::Recargar => {
                let firma = firma();
                if firma != self.firma || store::activo() != self.activo {
                    self.firma = firma;
                    self.recargar();
                }
            }
            Message::ModoOscuro(b) => self.editar(|p| p.modo_oscuro = b),
            Message::VisMostrar(b) => self.editar(|p| p.visualizador.mostrar = b),
            Message::VisEsquina(i) => self.editar(|p| p.visualizador.esquina = store::ESQUINAS[i].into()),
            Message::VisColores(i) => {
                self.editar(|p| p.visualizador.colores = if i == 1 { "manual" } else { "auto" }.into())
            }
            Message::VisBase(s) => self.editar(|p| p.visualizador.color_base = s),
            Message::VisPunta(s) => self.editar(|p| p.visualizador.color_punta = s),
            Message::RelojMostrar(b) => self.editar(|p| p.reloj.mostrar = b),
            Message::RelojEscala(v) => {
                if let Some(f) = self.fondos.get_mut(self.sel) {
                    f.perfil.reloj.escala = (v as f64 * 100.0).round() / 100.0;
                }
            }
            Message::RelojEscalaSoltar => self.editar(|_| {}),
            Message::RelojEstilo(i) => self.editar(|p| p.reloj.estilo = ESTILOS_RELOJ[i].into()),
            Message::RelojFuente(s) => self.editar(|p| p.reloj.texto.fuente = s),
            Message::RelojTextoColor(i, s) => self.editar(|p| {
                let t = &mut p.reloj.texto;
                *[&mut t.color_relleno, &mut t.color_borde, &mut t.color_sombra, &mut t.color_fondo][i] = s;
            }),
            Message::RelojPosicion(i) => self.editar(|p| p.reloj.posicion = store::POSICIONES_RELOJ[i].into()),
            Message::RelojEsquema(i) => self.editar(|p| p.reloj.esquema = store::ESQUEMAS_RELOJ[i].into()),
            Message::TemaAplicar(b) => self.editar(|p| p.tema.aplicar = b),
            Message::TemaColor(i, s) => self.editar(|p| {
                p.tema.colores.insert(crate::tema::PRINCIPALES[i].into(), s);
            }),
            Message::RelojColor(i, s) => {
                self.editar(|p| {
                    p.reloj.colores.insert(store::COLORES_RELOJ[i].into(), s);
                })
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.fondos.is_empty() {
            return widget::container(
                widget::column::with_capacity(4)
                    .push(widget::text::title3("El almacén está vacío"))
                    .push(widget::text::body(
                        "Añade uno desde la terminal:\ncosmic-ambient añadir <imagen> <nombre>",
                    ))
                    .spacing(12)
                    .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into();
        }

        widget::row::with_capacity(4)
            .push(self.galeria())
            .push(widget::scrollable(self.panel()).width(Length::Fixed(420.0)))
            .spacing(24)
            .padding(16)
            .into()
    }
}
