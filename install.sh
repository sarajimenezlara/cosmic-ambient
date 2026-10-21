#!/bin/sh
# Compila e instala cosmic-ambient para el usuario actual.
#   ./install.sh               instala en ~/.local (o en $PREFIX)
#   ./install.sh --uninstall   lo quita (tus fondos en ~/.local/share/cosmic-ambient se quedan)
set -eu

PREFIX=${PREFIX:-$HOME/.local}
BIN=$PREFIX/bin
APPS=${XDG_DATA_HOME:-$HOME/.local/share}/applications
AUTOSTART=${XDG_CONFIG_HOME:-$HOME/.config}/autostart
BINARIOS="cosmic-ambient ambient-visualizer ambient-clock"

if [ "${1:-}" = "--uninstall" ]; then
  pkill -x ambient-visuali 2>/dev/null || true
  pkill -x ambient-clock 2>/dev/null || true
  for b in $BINARIOS; do rm -f "$BIN/$b"; done
  rm -f "$APPS/io.github.sarajimenezlara.Ambient.desktop" \
        "$AUTOSTART/ambient-visualizer.desktop" "$AUTOSTART/ambient-clock.desktop"
  echo "Desinstalado. Tus fondos siguen en ~/.local/share/cosmic-ambient."
  exit 0
fi

cd "$(dirname "$0")"

falta=""
for p in cargo playerctl parec pactl curl fc-match; do
  command -v "$p" >/dev/null 2>&1 || falta="$falta $p"
done
if [ -n "$falta" ]; then
  echo "Faltan programas:$falta"
  echo "En Pop!_OS/Ubuntu: sudo apt install playerctl pulseaudio-utils curl fontconfig"
  echo "y Rust desde https://rustup.rs"
  exit 1
fi

cargo build --release

mkdir -p "$BIN" "$APPS" "$AUTOSTART"
for b in $BINARIOS; do
  install -m755 "target/release/$b" "$BIN/$b.nuevo"
  mv "$BIN/$b.nuevo" "$BIN/$b"
done
install -m644 data/io.github.sarajimenezlara.Ambient.desktop "$APPS/"
install -m644 data/ambient-visualizer.desktop data/ambient-clock.desktop "$AUTOSTART/"

echo
echo "Instalado. Abre «Ambient» en el lanzador de aplicaciones,"
echo "o añade un fondo desde la terminal: cosmic-ambient añadir <imagen> <nombre>"
