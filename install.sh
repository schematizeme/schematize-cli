#!/usr/bin/env bash
# install.sh — instalador do schematize (Linux-first: Debian/Mint/Ubuntu + openSUSE).
#
# PADRÃO: COMPILA NA MÁQUINA (do fonte). É open source e quem instala é dev — build
# local é o caminho de verdade: sem depender de CI/binário publicado, sempre casando
# com a arquitetura do host. O instalador cuida do Rust (rustup) e das libs de build
# da GUI (X11/Wayland/GL -dev, via apt/zypper/dnf — pede sudo).
#
# Uso:
#   curl -fsSL .../install.sh | bash                 # compila CLI + GUI na máquina (PADRÃO)
#   curl -fsSL .../install.sh | bash -s -- --binary  # atalho: binários pré-compilados do release (se houver)
#   curl -fsSL .../install.sh | bash -s -- --package # atalho: pacote .deb/.rpm da distro (se houver)
set -euo pipefail

REPO="schematizeme/schematize-cli"
BASE="https://github.com/$REPO/releases/latest/download"
API="https://api.github.com/repos/$REPO/releases/latest"
MODE="source"   # padrão: compilar do fonte
for a in "$@"; do case "$a" in
  --from-source|--source) MODE=source;; --binary) MODE=binary;;
  --package|--deb|--rpm) MODE=package;; --auto) MODE=package;;
  --gui) : ;;  # compat: no-op
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
# URL base de download da ÚLTIMA versão, via tag real da API (imune ao cache do
# CDN no asset de nome fixo em /latest/download/). Fallback: /latest/download.
resolve_dl() {
  local tag
  tag="$(curl -sfL -H 'Accept: application/vnd.github+json' -H 'User-Agent: schematize-install' "$API" 2>/dev/null \
        | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
  if [ -n "$tag" ]; then echo "https://github.com/$REPO/releases/download/$tag"; else echo "$BASE"; fi
}
# libs de runtime da GUI — só para o modo binário (o pacote resolve sozinho).
gui_runtime_deps() {
  case "$FAMILY" in
    debian) pkg_install libx11-6 libxcursor1 libxrandr2 libxi6 libxkbcommon0 libwayland-client0 libgl1 libfontconfig1 || true ;;
    rpm)    pkg_install libX11-6 libXcursor1 libXrandr2 libXi6 libxkbcommon0 libwayland-client0 Mesa-libGL1 fontconfig || true ;;
  esac
}
# libs de BUILD — só para --from-source.
gui_build_deps() {
  log "instalando libs de build da GUI (X11/Wayland/GL + fontconfig p/ o Slint)"
  case "$FAMILY" in
    # libfontconfig1-dev: o Slint 1.17 (fontique no núcleo) LINKA a libfontconfig no build.
    debian) pkg_install build-essential pkg-config libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
              libxkbcommon-dev libwayland-dev libgl1-mesa-dev libxcb1-dev libxcb-render0-dev \
              libxcb-shape0-dev libxcb-xfixes0-dev libfontconfig1-dev ;;
    rpm)    pkg_install gcc gcc-c++ make pkg-config libX11-devel libXcursor-devel libXrandr-devel \
              libXi-devel libxkbcommon-devel wayland-devel Mesa-libGL-devel libxcb-devel fontconfig-devel ;;
    *) die "GUI do fonte: instale manualmente as libs de X11/Wayland/GL/fontconfig da sua distro." ;;
  esac
}
# Fontes de cobertura ampla (CJK/árabe/devanagari/tailandês/bengali) — pra GUI não
# mostrar "quadradinhos" nos idiomas não-latinos. Best-effort (nomes variam por distro).
# No Debian, fonts-noto-core já traz Thai e Bengali (NotoSansThai/NotoSansBengali);
# fonts-noto (amplo) entra como reforço. Coreano vem do CJK; persa usa a escrita árabe.
ensure_fonts() {
  case "$FAMILY" in
    debian) pkg_install fonts-noto-core fonts-noto-cjk fonts-dejavu-core >/dev/null 2>&1 || true
            pkg_install fonts-noto >/dev/null 2>&1 || true ;;
    rpm)    if command -v zypper >/dev/null; then
              $SUDO zypper --non-interactive install -y noto-sans-fonts noto-sans-cjk-fonts dejavu-fonts >/dev/null 2>&1 || true
              $SUDO zypper --non-interactive install -y google-noto-sans-thai-fonts google-noto-sans-bengali-fonts >/dev/null 2>&1 || true
            else
              $SUDO dnf install -y google-noto-sans-fonts google-noto-sans-cjk-fonts dejavu-sans-fonts >/dev/null 2>&1 || true
              $SUDO dnf install -y google-noto-sans-thai-fonts google-noto-sans-bengali-fonts >/dev/null 2>&1 || true
            fi ;;
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
  # CAMINHO ABSOLUTO do schematize-gui (o Slint recém-instalado) — o `Exec=schematize gui`
  # dependia do PATH do ambiente gráfico, que muitas vezes NÃO tem ~/.cargo/bin; aí o DE
  # abria o schematize-gui do pacote em /usr/bin (egui, layout velho). Absoluto mata isso.
  local guibin; guibin="$(command -v schematize-gui 2>/dev/null || echo "$HOME/.cargo/bin/schematize-gui")"
  cat > "$app/schematize-gui.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=schematize
GenericName=Ecossistema schematize
Comment=Skills, overdev e mais — schematize
Exec=$guibin
Terminal=false
Categories=Development;Utility;
Keywords=schematize;skills;overdev;claude;
EOF
  update-desktop-database "$app" 2>/dev/null || true
  # No modo fonte, remove o lançador DUPLICADO do pacote (Exec=schematize-gui, que pode cair
  # no egui do /usr/bin) pra o DE usar só este (absoluto → Slint).
  if [ "${MODE:-source}" = source ] && [ -f /usr/share/applications/schematize-gui.desktop ]; then
    $SUDO rm -f /usr/share/applications/schematize-gui.desktop 2>/dev/null || true
    update-desktop-database /usr/share/applications 2>/dev/null || true
  fi
}

# Instala o schematize-updater (gestor de versão SEPARADO do app, cross-OS). Best-effort: nunca
# falha o install do app. Assim toda instalação/update do app já carrega o updater — quem atualizou
# passa a ter também o updater novo, sem rodar o bootstrap dele à mão.
install_updater() {
  local os arch asset
  os="$(uname -s)"; arch="$(uname -m)"
  case "$os/$arch" in
    Linux/x86_64)  asset="schematize-updater-linux-x86_64" ;;
    Darwin/arm64)  asset="schematize-updater-macos-arm64" ;;
    Darwin/x86_64) asset="schematize-updater-macos-x86_64" ;;
    *) return 0 ;;
  esac
  local url="https://github.com/schematizeme/schematize-updater/releases/latest/download/$asset"
  local dst="$HOME/.cargo/bin/schematize-updater"
  if curl -fsSL -o "$dst" "$url" 2>/dev/null && [ -s "$dst" ]; then
    chmod +x "$dst" 2>/dev/null || true
    ok "schematize-updater instalado ($dst) — atualize com: schematize-updater update"
  fi
}
post_config() {
  hash -r 2>/dev/null || true
  local BIN; BIN="$(command -v schematize || true)"
  # Só no modo pacote/binário: um schematize em ~/.cargo/bin sombreia o do pacote.
  # No modo source (padrão), ~/.cargo/bin É onde instalamos — não mexer.
  if [ "$MODE" != source ] && [ "$BIN" = "$HOME/.cargo/bin/schematize" ] && [ -x /usr/bin/schematize ] && command -v cargo >/dev/null; then
    log "removendo schematize antigo do ~/.cargo/bin (sombreava o pacote)"
    cargo uninstall schematize >/dev/null 2>&1 || rm -f "$HOME/.cargo/bin/schematize" "$HOME/.cargo/bin/schematize-gui"
    hash -r 2>/dev/null || true
    BIN="$(command -v schematize || true)"
  fi
  # No modo source (padrão), o .deb/.rpm do schematize (se existir) CONFLITA com a instalação de
  # fonte: dois binários (/usr/bin vs ~/.cargo/bin), dois launchers e autostart duplicado — a causa
  # dos conflitos de PATH no ambiente gráfico. Remove o pacote — a fonte é a verdade única.
  if [ "$MODE" = source ]; then
    if command -v dpkg >/dev/null && dpkg -l schematize 2>/dev/null | grep -q '^ii'; then
      log "removendo o pacote .deb antigo do schematize (conflitava com a instalação de fonte)"
      $SUDO apt-get remove -y schematize >/dev/null 2>&1 || $SUDO dpkg -r schematize >/dev/null 2>&1 || \
        warn "não removi o .deb automaticamente — rode: sudo apt remove schematize"
    elif command -v rpm >/dev/null && rpm -q schematize >/dev/null 2>&1; then
      log "removendo o pacote .rpm antigo do schematize (conflitava com a instalação de fonte)"
      { command -v zypper >/dev/null && $SUDO zypper -n rm schematize; } >/dev/null 2>&1 \
        || $SUDO dnf -y remove schematize >/dev/null 2>&1 \
        || warn "não removi o .rpm automaticamente — rode: sudo zypper rm schematize (ou dnf remove)"
    fi
    # limpa launcher/autostart residuais do pacote, se sobrarem (o Exec do pacote cai no /usr/bin)
    $SUDO rm -f /usr/share/applications/schematize-gui.desktop /etc/xdg/autostart/schematize-agent.desktop 2>/dev/null || true
    hash -r 2>/dev/null || true
    BIN="$(command -v schematize || true)"
  fi
  [ -n "$BIN" ] || { log "reabra o shell (PATH) e rode: schematize --help"; return; }
  ok "schematize $($BIN --version 2>/dev/null | awk '{print $2}') em $BIN"
  if [ "$BIN" != "/usr/bin/schematize" ] && [ -x /usr/bin/schematize ]; then
    log "aviso: '$BIN' está na frente de /usr/bin/schematize no PATH (shadow). Pra usar o do pacote: cargo uninstall schematize"
  fi
  "$BIN" autostart enable || true
  install_updater || true
  echo; ok "pronto. Próximos passos:"
  echo "    schematize skills install --all   # instala as skills"
  echo "    schematize overdev enable     # liga o modo overdev"
  echo "    schematize gui                # abre a janela (o & libera o terminal; ou use o menu de apps)"
  echo "    schematize skills list            # versões instaladas vs latest"
}

install_binary() {
  ensure_runtime_deps; gui_runtime_deps; ensure_fonts
  local DL; DL="$(resolve_dl)"
  local dst; if [ -n "$SUDO" ] || [ -w /usr/local/bin ]; then dst="/usr/local/bin"; else dst="$HOME/.local/bin"; fi
  mkdir -p "$dst" 2>/dev/null || $SUDO mkdir -p "$dst"
  local mv_="mv"; [ -w "$dst" ] || mv_="$SUDO mv"
  for pair in "schematize-linux-x86_64:schematize" "schematize-gui-linux-x86_64:schematize-gui"; do
    local src="${pair%%:*}" name="${pair##*:}" t
    log "baixando $name → $dst/$name"; t="$(mktemp)"
    curl -fSL -o "$t" "$DL/$src"; chmod 755 "$t"; $mv_ "$t" "$dst/$name"
  done
  install_gui_launcher
  case ":$PATH:" in *":$dst:"*) : ;; *) echo "  ⚠ adicione ao PATH: export PATH=\"$dst:\$PATH\"" ;; esac
  post_config
}
install_deb() {
  ensure_runtime_deps; ensure_fonts
  local DL; DL="$(resolve_dl)"
  local t; t="$(mktemp --suffix=.deb)"; log "baixando .deb (CLI + GUI)"; curl -fSL -o "$t" "$DL/schematize_amd64.deb"
  chmod 644 "$t"   # deixa o _apt (sandbox de download) ler o arquivo — sem o aviso de permissão
  log "instalando via apt (resolve libs da GUI)"; $SUDO apt-get install -y "$t" || { $SUDO dpkg -i "$t"; $SUDO apt-get -f install -y; }
  rm -f "$t"; post_config
}
install_rpm() {
  ensure_fonts
  local DL; DL="$(resolve_dl)"
  local t; t="$(mktemp --suffix=.rpm)"; log "baixando .rpm (CLI + GUI)"; curl -fSL -o "$t" "$DL/schematize.x86_64.rpm"; chmod 644 "$t"
  if command -v zypper >/dev/null; then $SUDO zypper --non-interactive install -y --allow-unsigned-rpm "$t"; else $SUDO dnf install -y "$t"; fi
  rm -f "$t"; post_config
}
# Deixa um checkout na versão do main (fetch+reset se já existe; clona se não). Shallow.
_sync_repo() { # <url> <dir>
  local url="$1" dir="$2"
  if [ -d "$dir/.git" ]; then
    git -C "$dir" fetch --depth 1 origin main 2>/dev/null && git -C "$dir" reset --hard origin/main 2>/dev/null && return 0
    rm -rf "$dir"
  fi
  git clone --depth 1 "$url" "$dir"
}
install_source() {
  ensure_runtime_deps; ensure_rust; gui_build_deps; ensure_fonts
  . "$HOME/.cargo/env" 2>/dev/null || true
  export PATH="$HOME/.cargo/bin:$PATH"
  # Checkouts PERSISTENTES: o `target/` fica cacheado entre updates, então `cargo build` só
  # recompila o que MUDOU (as deps pesadas tipo Slint não recompilam). NÃO usa `cargo install
  # --force`, que jogava fora o cache e reconstruía a árvore inteira toda vez. 1ª vez é cheia;
  # updates depois são incrementais (segundos em vez de minutos).
  local base="$HOME/.schematize/src"; mkdir -p "$base"
  local cli="$base/schematize-cli" gui="$base/schematize_gui_slint"

  log "compilando o CLI do fonte (incremental — recompila só o que mudou; 1ª vez leva minutos)"
  _sync_repo "https://github.com/$REPO.git" "$cli" || die "clone do CLI falhou"
  ( cd "$cli" && cargo build --release --features gui ) || die "build do CLI falhou"
  install -m755 "$cli/target/release/schematize" "$HOME/.cargo/bin/schematize"
  # a GUI egui vem junto como fallback de `schematize-gui` (o Slint sobrescreve abaixo se compilar).
  [ -f "$cli/target/release/schematize-gui" ] && install -m755 "$cli/target/release/schematize-gui" "$HOME/.cargo/bin/schematize-gui"
  hash -r 2>/dev/null || true

  # GUI DEFAULT = Slint (repo próprio). Sobrescreve o egui; se o build falhar, o egui fica de fallback.
  log "compilando a GUI Slint — schematize-gui (incremental)"
  if _sync_repo "https://github.com/schematizeme/schematize_gui_slint.git" "$gui" 2>/dev/null \
     && ( cd "$gui" && cargo build --release ) \
     && install -m755 "$gui/target/release/schematize-gui" "$HOME/.cargo/bin/schematize-gui"; then
    ok "GUI Slint instalada (schematize-gui)."
  else
    warn "build da GUI Slint falhou — schematize-gui anterior (ou egui) mantido. Rode de novo depois."
  fi
  install_gui_launcher
  post_config
}

case "$MODE" in
  source) install_source ;;
  binary) install_binary ;;
  package)
    case "$FAMILY" in
      debian) command -v apt-get >/dev/null && install_deb || install_binary ;;
      rpm)    (command -v zypper >/dev/null || command -v dnf >/dev/null) && install_rpm || install_binary ;;
      *)      install_binary ;;
    esac ;;
  *) install_source ;;
esac
