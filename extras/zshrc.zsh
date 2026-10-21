# Colores de cosmic-ambient para zsh-syntax-highlighting, zsh-autosuggestions y fzf.
# Pégalo en tu ~/.zshrc antes de cargar zsh-syntax-highlighting y activa
# `zsh = true` en ~/.config/cosmic-ambient/integraciones.toml.
# Se vuelve a leer antes de cada prompt si ha cambiado: al cambiar de fondo,
# la terminal abierta cambia de colores sin reiniciarla.
zmodload -F zsh/stat b:zstat
_ambient_tema=~/.config/cosmic-ambient/theme.zsh
_ambient_cargar() {
  [[ -r $_ambient_tema ]] || return
  local m; m=$(zstat +mtime $_ambient_tema 2>/dev/null)
  [[ $m == $_ambient_mtime ]] && return
  _ambient_mtime=$m
  source $_ambient_tema
}
precmd_functions+=(_ambient_cargar)
_ambient_cargar
