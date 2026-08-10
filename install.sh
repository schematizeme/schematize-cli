#!/usr/bin/env bash
# install.sh — instalador do schematize (Linux-first: Debian/Mint/Ubuntu + openSUSE).
#
# A GUI (schematize-gui) já vem PRONTA no .deb/.rpm e no binário — sem compilar,
# sem libs -dev. As libs de runtime (X11/GL/Wayland) o apt/zypper resolvem, e num
# desktop KDE/Cinnamon já estão presentes.
#
# Uso:
#   curl -fsSL .../install.sh | bash                 # CLI + GUI conforme a distro (.deb/.rpm/binário)
#   curl -fsSL .../install.sh | bash -s -- --binary  # binários pré-compilados (CLI + GUI), sem pacote
#   curl -fsSL .../install.sh | bash -s -- --from-source   # compila na máquina (instala libs de build)
set -euo pipefail

REPO="schematizeme/schematize-cli"
BASE="https://github.com/$REPO/releases/latest/download"
MODE="auto"
for a in "$@"; do case "$a" in
  --from-source) MODE=source;; --binary) MODE=binary;; --auto) MODE=auto;;
  --gui) : ;;  # compat: a GUI já vem por padrão; flag mantida como no-op
esac; done

log() { printf '\033[1;36m▶ %s\033[0m\n' "$*"; }
ok()  { printf '\033[1;32m✓ %s\033[0m\n' "$*"; }
die() { printf '\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || die "só Linux por enquanto."
SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO="sudo"

. /etc/os-release 2>/dev/null || true
FAMILY="unknown"
case " ${ID:-} ${ID_LIKE:-} " in
  *" debian "*|*" ubuntu "*|*" linuxmint "*) FAMILY="debian" ;;
  *" suse "*|*" opensuse "*|*" sles "*|*" fedora "*|*" rhel "*) FAMILY="rpm" ;;
esac
log "distro: ${PRETTY_NAME:-desconhecida} (família: $FAMILY) — modo: $MODE"

pkg_install() {
  case "$FAMILY" in
    debian) $SUDO apt-get update -qq && $SUDO apt-get install -y "$@" ;;
    rpm)    if command -v zypper >/dev/null; then $SUDO zypper --non-interactive install -y "$@"; else $SUDO dnf install -y "$@"; fi ;;
    *) log "instale manualmente: $*" ;;
  esac
}
ensure_runtime_deps() {
  local miss=(); for b in curl unzip git; do command -v "$b" >/dev/null || miss+=("$b"); done
  [ ${#miss[@]} -gt 0 ] && pkg_install "${miss[@]}" || true
}
# libs de runtime da GUI — só para o modo binário (o pacote resolve sozinho).
gui_runtime_deps() {
  case "$FAMILY" in
    debian) pkg_install libx11-6 libxcursor1 libxrandr2 libxi6 libxkbcommon0 libwayland-client0 libgl1 || true ;;
    rpm)    pkg_install libX11-6 libXcursor1 libXrandr2 libXi6 libxkbcommon0 libwayland-client0 Mesa-libGL1 || true ;;
  esac
}
# libs de BUILD — só para --from-source.
gui_build_deps() {
  log "instalando libs de build da GUI (X11/Wayland/GL)"
  case "$FAMILY" in
    debian) pkg_install build-essential pkg-config libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
              libxkbcommon-dev libwayland-dev libgl1-mesa-dev libxcb1-dev libxcb-render0-dev \
              libxcb-shape0-dev libxcb-xfixes0-dev ;;
    rpm)    pkg_install gcc gcc-c++ make pkg-config libX11-devel libXcursor-devel libXrandr-devel \
              libXi-devel libxkbcommon-devel wayland-devel Mesa-libGL-devel libxcb-devel ;;
    *) die "GUI do fonte: instale manualmente as libs de X11/Wayland/GL da sua distro." ;;
  esac
}
ensure_rust() {
  if ! command -v cargo >/dev/null; then
    log "instalando Rust (rustup)"; curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
    . "$HOME/.cargo/env"
  fi
}
install_gui_launcher() {
  local app="$HOME/.local/share/applications"; mkdir -p "$app"
  cat > "$app/schematize-gui.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=schematize
GenericName=Gerenciador de skills
Comment=Gerenciar skills e overdev do schematize
Exec=schematize-gui
Terminal=false
Categories=Development;Utility;
Keywords=schematize;skills;overdev;claude;
EOF
}

post_config() {
  hash -r 2>/dev/null || true
  local BIN; BIN="$(command -v schematize || true)"
  [ -n "$BIN" ] || { log "reabra o shell (PATH) e rode: schematize --help"; return; }
  ok "schematize $($BIN --version 2>/dev/null | awk '{print $2}') em $BIN"
  if [ "$BIN" != "/usr/bin/schematize" ] && [ -x /usr/bin/schematize ]; then
    log "aviso: '$BIN' está na frente de /usr/bin/schematize no PATH (shadow). Pra usar o do pacote: cargo uninstall schematize"
  fi
  "$BIN" autostart enable || true
  echo; ok "pronto. Próximos passos:"
  echo "    schematize install --all      # instala as skills"
  echo "    schematize overdev enable     # liga o modo overdev"
  echo "    schematize-gui                # abre a janela (também no menu de apps)"
  echo "    schematize list               # versões instaladas vs latest"
}

install_binary() {
  ensure_runtime_deps; gui_runtime_deps
  local dst; if [ -n "$SUDO" ] || [ -w /usr/local/bin ]; then dst="/usr/local/bin"; else dst="$HOME/.local/bin"; fi
  mkdir -p "$dst" 2>/dev/null || $SUDO mkdir -p "$dst"
  local mv_="mv"; [ -w "$dst" ] || mv_="$SUDO mv"
  for pair in "schematize-linux-x86_64:schematize" "schematize-gui-linux-x86_64:schematize-gui"; do
    local src="${pair%%:*}" name="${pair##*:}" t
    log "baixando $name → $dst/$name"; t="$(mktemp)"
    curl -fSL -o "$t" "$BASE/$src"; chmod 755 "$t"; $mv_ "$t" "$dst/$name"
  done
  install_gui_launcher
  case ":$PATH:" in *":$dst:"*) : ;; *) echo "  ⚠ adicione ao PATH: export PATH=\"$dst:\$PATH\"" ;; esac
  post_config
}
install_deb() {
  ensure_runtime_deps
  local t; t="$(mktemp --suffix=.deb)"; log "baixando .deb (CLI + GUI)"; curl -fSL -o "$t" "$BASE/schematize_amd64.deb"
  chmod 644 "$t"   # deixa o _apt (sandbox de download) ler o arquivo — sem o aviso de permissão
  log "instalando via apt (resolve libs da GUI)"; $SUDO apt-get install -y "$t" || { $SUDO dpkg -i "$t"; $SUDO apt-get -f install -y; }
  rm -f "$t"; post_config
}
install_rpm() {
  local t; t="$(mktemp --suffix=.rpm)"; log "baixando .rpm (CLI + GUI)"; curl -fSL -o "$t" "$BASE/schematize.x86_64.rpm"; chmod 644 "$t"
  if command -v zypper >/dev/null; then $SUDO zypper --non-interactive install -y --allow-unsigned-rpm "$t"; else $SUDO dnf install -y "$t"; fi
  rm -f "$t"; post_config
}
install_source() {
  ensure_runtime_deps; ensure_rust; gui_build_deps
  local d; d="$(mktemp -d)"; log "clonando + compilando (CLI + GUI)"
  git clone --depth 1 "https://github.com/$REPO.git" "$d/src"
  ( cd "$d/src" && cargo install --path . --features gui )
  install_gui_launcher
  rm -rf "$d"; post_config
}

case "$MODE" in
  source) install_source ;;
  binary) install_binary ;;
  auto)
    case "$FAMILY" in
      debian) command -v apt-get >/dev/null && install_deb || install_binary ;;
      rpm)    (command -v zypper >/dev/null || command -v dnf >/dev/null) && install_rpm || install_binary ;;
      *)      install_binary ;;
    esac ;;
esac
