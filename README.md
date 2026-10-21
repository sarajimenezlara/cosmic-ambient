# Ambient

**Fondos de escritorio para [COSMIC](https://system76.com/cosmic) con su propio ambiente.**
Cada fondo guarda su configuración: al ponerlo, cambian también los colores
del sistema, el reloj del escritorio y el visualizador de música.

*Wallpapers for the COSMIC desktop that bring their own vibe: each wallpaper
keeps its own system colors, desktop clock and music visualizer, and switching
wallpapers switches all of it.*

## Qué hace

- **Galería de fondos**, cada uno con su perfil (`perfil.toml`).
- **Estética del sistema**: una paleta de 26 colores por fondo (con los nombres
  de [Catppuccin](https://catppuccin.com)) que tiñe el tema de COSMIC: panel,
  dock, ventanas, color de acento, la terminal de COSMIC y las apps GTK.
  Opcionalmente también zsh, fzf, [starship](https://starship.rs), bat, btop,
  VS Code y Neovim, que cambian en caliente.
- **Reloj de escritorio** (`ambient-clock`): hora y fecha con la tipografía que
  quieras, relleno, borde y sombra, o sobre una tarjeta de color redondeada.
  Si tienes instalado el Moonlight Clock, también se puede usar ese.
- **Visualizador de música** (`ambient-visualizer`): carátula, título, artista,
  barra de progreso y barras de audio, con colores sacados del fondo o
  elegidos a mano. Funciona con cualquier reproductor MPRIS (Spotify,
  navegadores, …).

Todo va en la capa de escritorio: no tapa ventanas ni recibe clics.

## Requisitos

- Pop!_OS 24.04 (o cualquier distribución) con el escritorio **COSMIC** en Wayland.
- [Rust](https://rustup.rs) para compilar.
- `playerctl`, `parec`/`pactl` (PulseAudio o PipeWire), `curl` y `fc-match`:

  ```sh
  sudo apt install playerctl pulseaudio-utils curl fontconfig
  ```

## Instalación

```sh
git clone https://github.com/sarajimenezlara/cosmic-ambient.git
cd cosmic-ambient
./install.sh
```

Instala `cosmic-ambient`, `ambient-visualizer` y `ambient-clock` en
`~/.local/bin`, la app «Ambient» en el lanzador, y el reloj y el visualizador
en el inicio de sesión. Para quitarlo: `./install.sh --uninstall`.

## Uso

Abre **Ambient** desde el lanzador, o usa la terminal:

```sh
cosmic-ambient añadir ~/Imágenes/fondo.png "Mi fondo"   # meterlo en el almacén
cosmic-ambient lista                                     # * = el puesto
cosmic-ambient aplicar "Mi fondo"
cosmic-ambient perfil "Mi fondo"                         # ruta de su perfil.toml
cosmic-ambient cambiar-imagen "Mi fondo" otra.png
cosmic-ambient quitar "Mi fondo"
```

Un fondo nuevo empieza con la configuración del que está puesto. Los cambios
que hagas en la ventana se guardan y, si es el fondo puesto, se ven al momento.

### El perfil

Cada fondo vive en `~/.local/share/cosmic-ambient/<id>/` con su imagen y un
`perfil.toml` como este (resumido):

```toml
nombre = "Mi fondo"
modo_oscuro = true

[visualizador]
mostrar = true
esquina = "abajo-izquierda"
colores = "auto"            # o "manual" con color_base y color_punta

[reloj]
mostrar = true
estilo = "texto"            # o "moonlight"
posicion = "top left"
escala = 1.0

[reloj.texto]
fuente = "Playfair Display" # cualquier fuente instalada
color_relleno = "#ffffff"
color_borde = "#1e1e2e"
color_fondo = ""            # un color para ponerlo sobre una tarjeta

[tema]
aplicar = true

[tema.colores]              # los que falten salen de Catppuccin Mocha
mauve = "#e0409a"           # acento
base = "#170d15"            # fondo de las ventanas
text = "#f3e4ec"
# … rosewater, flamingo, pink, red, maroon, peach, yellow, green, teal, sky,
# sapphire, blue, lavender, subtext1/0, overlay2/1/0, surface2/1/0, mantle, crust
```

### Integraciones

El tema de COSMIC se tiñe siempre que el fondo tenga `[tema] aplicar = true`.
Lo demás toca la configuración de otros programas, así que viene apagado. Se
activa en `~/.config/cosmic-ambient/integraciones.toml` (se crea solo):

| Integración | Qué hace | Además |
|---|---|---|
| `zsh` | Colores de zsh-syntax-highlighting, autosuggestions y fzf | Pega [`extras/zshrc.zsh`](extras/zshrc.zsh) en tu `.zshrc` |
| `starship` | Paleta `ambient` con los 26 colores | Usa esos nombres en tus estilos (`style = "mauve"`) |
| `bat` | Tema «Ambient» | — |
| `btop` | Tema «ambient» | — |
| `vscode` | Cambia los colores del tema Catppuccin Mocha | Extensión Catppuccin instalada |
| `neovim` | Escribe la paleta para Catppuccin, que se recolorea en caliente | Copia [`extras/nvim/ambient.lua`](extras/nvim/ambient.lua) a `lua/plugins/` (lazy.nvim) |

## Estructura

| Carpeta | Qué es |
|---|---|
| `ambient/` | La app y la línea de comandos (Rust + [libcosmic](https://github.com/pop-os/libcosmic)) |
| `ambient-visualizer/` | El visualizador (Wayland layer-shell, FFT del audio que suena) |
| `ambient-clock/` | El reloj de texto (Wayland layer-shell) |
| `extras/` | Fragmentos para zsh y Neovim |

## Licencia

[MIT](LICENSE). Las plantillas de `ambient/templates/` son de
[Catppuccin](https://github.com/catppuccin) (MIT, ver
[su licencia](ambient/templates/LICENSE-catppuccin)).
