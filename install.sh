#!/usr/bin/env bash
# install.sh — instalador do schematize CLI (Linux-first: Debian/Mint/Ubuntu + openSUSE).
#
# Uso:
#   curl -fsSL https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh | bash
#   ... | bash -s -- --from-source     # compila do fonte na máquina
#   ... | bash -s -- --binary          # baixa só o binário estático (sem pacote)
#
# Auto (default): usa .deb no apt, .rpm no zypper; senão binário. Instala deps e configura
# o agente de atualização (autostart). Idempotente.
set -euo pipefail

REPO="schematizeme/schematize-cli"
BASE="https://github.com/$REPO/releases/latest/download"
MODE="auto"
for a in "$@"; do case "$a" in --from-source) MODE=source;; --binary) MODE=binary;; --auto) MODE=auto;; esac; done

log() { printf '\033[1;36m▶ %s\033[0m\n' "$*"; }
ok()  { printf '\033[1;32m✓ %s\033[0m\n' "$*"; }
die() { printf '\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || die "só Linux por enquanto."
SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO="sudo"

# --- distro ---
. /etc/os-release 2>/dev/null || true
FAMILY="unknown"
case " ${ID:-} ${ID_LIKE:-} " in
  *" debian "*|*" ubuntu "*|*" linuxmint "*) FAMILY="debian" ;;
  *" suse "*|*" opensuse "*|*" sles "*|*" fedora "*|*" rhel "*) FAMILY="rpm" ;;
esac
log "distro: ${PRETTY_NAME:-desconhecida} (família: $FAMILY) — modo: $MODE"

pkg_install() { # instala pacotes de sistema (deps)
  case "$FAMILY" in
    debian) $SUDO apt-get update -qq && $SUDO apt-get install -y "$@" ;;
    rpm)    if command -v zypper >/dev/null; then $SUDO zypper --non-interactive install -y "$@"; else $SUDO dnf install -y "$@"; fi ;;
    *) log "instale manualmente: $*" ;;
  esac
}

ensure_runtime_deps() {
  local miss=()
  for b in curl unzip git; do command -v "$b" >/dev/null || miss+=("$b"); done
  [ ${#miss[@]} -gt 0 ] && { log "instalando deps: ${miss[*]}"; pkg_install "${miss[@]}"; } || true
}

post_config() {
  hash -r 2>/dev/null || true
  local BIN; BIN="$(command -v schematize || true)"
  [ -n "$BIN" ] || { log "schematize instalado; reabra o shell (PATH) e rode: schematize --help"; return; }
  ok "schematize $($BIN --version 2>/dev/null | awk '{print $2}') instalado em $BIN"
  "$BIN" autostart enable || true
  echo
  ok "pronto. Próximos passos:"
  echo "    schematize install --all      # instala as skills"
  echo "    schematize overdev enable     # liga o modo overdev (dev contínuo)"
  echo "    schematize list               # versões instaladas vs latest"
}

install_binary() {
  ensure_runtime_deps
  local dst; if [ -n "$SUDO" ] || [ -w /usr/local/bin ]; then dst="/usr/local/bin"; else dst="$HOME/.local/bin"; fi
  mkdir -p "$dst" 2>/dev/null || $SUDO mkdir -p "$dst"
  log "baixando binário → $dst/schematize"
  local tmp; tmp="$(mktemp)"; curl -fSL -o "$tmp" "$BASE/schematize-linux-x86_64"
  chmod +x "$tmp"
  if [ -w "$dst" ]; then mv "$tmp" "$dst/schematize"; else $SUDO mv "$tmp" "$dst/schematize"; fi
  case ":$PATH:" in *":$dst:"*) : ;; *) echo "  ⚠ adicione ao PATH: export PATH=\"$dst:\$PATH\"" ;; esac
  post_config
}

install_deb() {
  ensure_runtime_deps
  local tmp; tmp="$(mktemp --suffix=.deb)"
  log "baixando .deb"; curl -fSL -o "$tmp" "$BASE/schematize_amd64.deb"
  log "instalando via apt (resolve deps)"; $SUDO apt-get install -y "$tmp" || $SUDO dpkg -i "$tmp" || { $SUDO apt-get -f install -y; }
  rm -f "$tmp"; post_config
}

install_rpm() {
  local tmp; tmp="$(mktemp --suffix=.rpm)"
  log "baixando .rpm"; curl -fSL -o "$tmp" "$BASE/schematize.x86_64.rpm"
  if command -v zypper >/dev/null; then $SUDO zypper --non-interactive install -y --allow-unsigned-rpm "$tmp"; else $SUDO dnf install -y "$tmp"; fi
  rm -f "$tmp"; post_config
}

install_source() {
  log "compilando do fonte"
  ensure_runtime_deps
  command -v cc >/dev/null || pkg_install gcc || true
  if ! command -v cargo >/dev/null; then
    log "instalando Rust (rustup)"; curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
    . "$HOME/.cargo/env"
  fi
  local d; d="$(mktemp -d)"; git clone --depth 1 "https://github.com/$REPO.git" "$d/src"
  ( cd "$d/src" && cargo install --path . )
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
