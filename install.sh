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

# "PREVER MACACOS": software de massa não pode quebrar porque o usuário rodou como root (su/sudo).
# Se estamos como root, descobre o usuário REAL (mesmo sob su) e instala PRA ELE — o app é de
# usuário (mora em ~/.cargo/bin, a GUI dele que abre). Só as libs do apt rodam como root.
REAL_USER=""
if [ "$(id -u)" -eq 0 ]; then
  REAL_USER="${SUDO_USER:-}"
  [ -z "$REAL_USER" ] && REAL_USER="$(logname 2>/dev/null || true)"
  [ -z "$REAL_USER" ] && REAL_USER="$(stat -c %U "$(tty 2>/dev/null)" 2>/dev/null || true)"
  [ "$REAL_USER" = "root" ] && REAL_USER=""
fi
if [ -n "$REAL_USER" ]; then
  TARGET_HOME="$(getent passwd "$REAL_USER" | cut -d: -f6)"
  [ -n "$TARGET_HOME" ] || TARGET_HOME="/home/$REAL_USER"
  ok "detectei root — instalando pro seu usuário '$REAL_USER' (HOME=$TARGET_HOME), não pro /root."
else
  TARGET_HOME="$HOME"
fi
# Roda um comando COMO o usuário real (se root com usuário detectado); senão direto. Já leva o
# ~/.cargo/bin do usuário no PATH (rustup/cargo) e o HOME certo.
as_user() {
  if [ -n "$REAL_USER" ]; then
    sudo -u "$REAL_USER" -H env "HOME=$TARGET_HOME" "PATH=$TARGET_HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin" "$@"
  else
    env "PATH=$TARGET_HOME/.cargo/bin:$PATH" "$@"
  fi
}

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
  # Tudo como o usuário REAL (rustup/cargo moram no HOME dele, não no /root).
  if ! as_user sh -c 'command -v cargo >/dev/null 2>&1'; then
    log "instalando Rust (rustup) para ${REAL_USER:-você}"
    as_user sh -c 'curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal'
  fi
  # rustup pode ter o shim `cargo` mas SEM toolchain default (1ª vez, ou HOME sem ~/.rustup) → o
  # cargo falha com "no default configured". Garante um default estável, senão o build quebra.
  if as_user sh -c 'command -v rustup >/dev/null 2>&1' && ! as_user sh -c 'rustup default >/dev/null 2>&1'; then
    log "configurando o toolchain default do Rust (stable)"
    as_user sh -c 'rustup default stable || { rustup toolchain install stable && rustup default stable; }'
  fi
}
install_gui_launcher() {
  local app="$TARGET_HOME/.local/share/applications"; as_user mkdir -p "$app"
  # CAMINHO ABSOLUTO do schematize-gui (Slint) no HOME do usuário — o `Exec=schematize gui` dependia
  # do PATH do ambiente gráfico (sem ~/.cargo/bin), aí o DE abria o egui velho. Absoluto mata isso.
  local guibin="$TARGET_HOME/.cargo/bin/schematize-gui"
  printf '%s\n' "[Desktop Entry]
Type=Application
Name=schematize
GenericName=Ecossistema schematize
Comment=Skills, overdev e mais — schematize
Exec=$guibin
Icon=schematize
Terminal=false
Categories=Development;Utility;
Keywords=schematize;skills;overdev;claude;
StartupWMClass=schematize-gui" | as_user tee "$app/schematize-gui.desktop" >/dev/null
  as_user update-desktop-database "$app" 2>/dev/null || true
  # No modo fonte, remove o lançador DUPLICADO do pacote (Exec=schematize-gui, que pode cair
  # no egui do /usr/bin) pra o DE usar só este (absoluto → Slint).
  if [ "${MODE:-source}" = source ] && [ -f /usr/share/applications/schematize-gui.desktop ]; then
    $SUDO rm -f /usr/share/applications/schematize-gui.desktop 2>/dev/null || true
    update-desktop-database /usr/share/applications 2>/dev/null || true
  fi
  install_app_icons
}

# Dimensões do ícone do app no padrão freedesktop (hicolor). O SVG master vai em scalable/.
ICON_SIZES="16 24 32 48 64 128 256 512 1024"

# Instala o ícone do app no tema hicolor do usuário (padrão freedesktop cross-desktop) — é o que
# faz o DE mostrar o ícone certo no dock/menu em vez do "W" de fallback. O .desktop referencia só
# o NOME `schematize`, então o ícone PRECISA morar aqui pra o tema resolver. Best-effort: nunca
# derruba o install. Fonte dos assets: o checkout do CLI (modo source) ou, se ausente, baixa do repo.
install_app_icons() {
  local ithome="$TARGET_HOME/.local/share/icons/hicolor"
  local src="$TARGET_HOME/.schematize/src/schematize-cli/assets/icons" n
  # RESILIENTE: gera os PNGs A PARTIR DO CÓDIGO (`schematize icon --hicolor`) — não depende de asset
  # commitado nem de rasterizador de SVG. Se o binário ainda não existir, cai no copy/download abaixo.
  local szbin="$TARGET_HOME/.cargo/bin/schematize"
  if [ -x "$szbin" ] && as_user "$szbin" icon --hicolor "$ithome" >/dev/null 2>&1; then
    ok "ícones do app gerados do código (todos os tamanhos)"
    if [ -f "$src/schematize.svg" ]; then
      as_user mkdir -p "$ithome/scalable/apps"
      as_user cp "$src/schematize.svg" "$ithome/scalable/apps/schematize.svg" 2>/dev/null || true
    fi
    as_user gtk-update-icon-cache -f -t "$ithome" 2>/dev/null || true
    as_user update-desktop-database "$TARGET_HOME/.local/share/applications" 2>/dev/null || true
    return 0
  fi
  if [ -d "$src/hicolor" ]; then
    # Modo source (padrão): copia do checkout já clonado em ~/.schematize/src/schematize-cli.
    for n in $ICON_SIZES; do
      [ -f "$src/hicolor/${n}x${n}/apps/schematize.png" ] || continue
      as_user mkdir -p "$ithome/${n}x${n}/apps"
      as_user cp "$src/hicolor/${n}x${n}/apps/schematize.png" "$ithome/${n}x${n}/apps/schematize.png" 2>/dev/null || true
    done
    if [ -f "$src/schematize.svg" ]; then
      as_user mkdir -p "$ithome/scalable/apps"
      as_user cp "$src/schematize.svg" "$ithome/scalable/apps/schematize.svg" 2>/dev/null || true
    fi
  else
    # Modo binário/pacote (sem checkout): baixa os PNGs + SVG do repo, best-effort.
    local raw="https://raw.githubusercontent.com/$REPO/main/assets/icons"
    for n in $ICON_SIZES; do
      as_user mkdir -p "$ithome/${n}x${n}/apps"
      as_user sh -c "curl -fsSL -o '$ithome/${n}x${n}/apps/schematize.png' '$raw/hicolor/${n}x${n}/apps/schematize.png'" 2>/dev/null || true
    done
    as_user mkdir -p "$ithome/scalable/apps"
    as_user sh -c "curl -fsSL -o '$ithome/scalable/apps/schematize.svg' '$raw/schematize.svg'" 2>/dev/null || true
  fi
  # Atualiza os caches do tema de ícones / lançadores (best-effort — o DE pode não ter as ferramentas).
  as_user gtk-update-icon-cache -f -t "$ithome" 2>/dev/null || true
  as_user update-desktop-database "$TARGET_HOME/.local/share/applications" 2>/dev/null || true
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
  local dst="$TARGET_HOME/.cargo/bin/schematize-updater"
  as_user mkdir -p "$TARGET_HOME/.cargo/bin"
  if as_user sh -c "curl -fsSL -o '$dst' '$url'" 2>/dev/null && [ -s "$dst" ]; then
    as_user chmod +x "$dst" 2>/dev/null || true
    ok "schematize-updater instalado ($dst) — atualize com: schematize-updater update"
  fi
}
post_config() {
  local BIN="$TARGET_HOME/.cargo/bin/schematize"
  # No modo source, o .deb/.rpm do schematize (se existir) CONFLITA (dois binários/launchers). Remove
  # (as libs do apt são root — $SUDO="" quando já root). A fonte no HOME do usuário é a verdade única.
  if [ "$MODE" = source ]; then
    if command -v dpkg >/dev/null && dpkg -l schematize 2>/dev/null | grep -q '^ii'; then
      log "removendo o pacote .deb antigo do schematize (conflitava com a fonte)"
      $SUDO apt-get remove -y schematize >/dev/null 2>&1 || $SUDO dpkg -r schematize >/dev/null 2>&1 || \
        warn "não removi o .deb — rode: sudo apt remove schematize"
    elif command -v rpm >/dev/null && rpm -q schematize >/dev/null 2>&1; then
      log "removendo o pacote .rpm antigo do schematize"
      { command -v zypper >/dev/null && $SUDO zypper -n rm schematize; } >/dev/null 2>&1 \
        || $SUDO dnf -y remove schematize >/dev/null 2>&1 \
        || warn "não removi o .rpm — rode: sudo zypper rm schematize (ou dnf remove)"
    fi
    $SUDO rm -f /usr/share/applications/schematize-gui.desktop /etc/xdg/autostart/schematize-agent.desktop 2>/dev/null || true
  fi
  [ -x "$BIN" ] || { warn "o binário não apareceu em $BIN — rode o install de novo."; return; }
  ok "schematize $(as_user "$BIN" --version 2>/dev/null | awk '{print $2}') em $BIN (usuário ${REAL_USER:-$USER})"
  as_user "$BIN" autostart enable || true
  install_updater || true
  echo; ok "pronto. Próximos passos:"
  echo "    schematize skills install --all   # instala as skills"
  echo "    schematize overdev enable         # liga o modo overdev"
  echo "    schematize gui                    # abre a janela (ou use o menu de apps)"
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
_sync_repo() { # <url> <dir>  — git como o usuário real (checkout no HOME dele)
  local url="$1" dir="$2"
  if [ -d "$dir/.git" ]; then
    as_user git -C "$dir" fetch --depth 1 origin main 2>/dev/null && as_user git -C "$dir" reset --hard origin/main 2>/dev/null && return 0
    rm -rf "$dir"
  fi
  as_user git clone --depth 1 "$url" "$dir"
}
install_source() {
  # Deps do apt como root ($SUDO); build/instalação como o USUÁRIO REAL (as_user + TARGET_HOME) —
  # o app mora no HOME do usuário, nunca em /root, mesmo que tenham rodado via su/sudo.
  ensure_runtime_deps; ensure_rust; gui_build_deps; ensure_fonts
  # Checkouts PERSISTENTES no HOME do usuário: `target/` cacheado → `cargo build` incremental (só o
  # que mudou recompila; deps pesadas tipo Slint não). Sem `cargo install --force` (que zerava o cache).
  local base="$TARGET_HOME/.schematize/src"; as_user mkdir -p "$base"
  local cli="$base/schematize-cli" gui="$base/schematize_gui_slint"
  local bin="$TARGET_HOME/.cargo/bin"; as_user mkdir -p "$bin"

  # CLI SEM a feature `gui` — NÃO produz o schematize-gui egui (a única GUI é o Slint, repo próprio).
  log "compilando o CLI do fonte (incremental — recompila só o que mudou; 1ª vez leva minutos)"
  _sync_repo "https://github.com/$REPO.git" "$cli" || die "clone do CLI falhou"
  as_user sh -c "cd '$cli' && cargo build --release" || die "build do CLI falhou"
  as_user install -m755 "$cli/target/release/schematize" "$bin/schematize"

  # GUI = Slint (a ÚNICA GUI). Se o build falhar, NÃO cai pro egui — melhor sem GUI que o fantasma.
  # A GUI depende do crate `schematize` como git-dep (branch=main); o Cargo.lock commitado FIXA um
  # commit e `git reset --hard` o restaura a cada update, então a versão embutida (`app_version()`)
  # ficava travada numa release velha. `cargo update -p schematize` avança o git-dep pro HEAD ANTES
  # de compilar (best-effort: offline segue com o lock). Sem isso, "atualizei mas abre versão antiga".
  log "compilando a GUI Slint — schematize-gui (incremental)"
  if _sync_repo "https://github.com/schematizeme/schematize_gui_slint.git" "$gui" 2>/dev/null \
     && { as_user sh -c "cd '$gui' && cargo update -p schematize" 2>/dev/null || true; } \
     && as_user sh -c "cd '$gui' && cargo build --release" \
     && as_user install -m755 "$gui/target/release/schematize-gui" "$bin/schematize-gui"; then
    ok "GUI Slint instalada (schematize-gui)."
    # Encerra GUI antiga ainda aberta — fechar a janela não matava o processo, e o relaunch reusava
    # a versão anterior. `pkill -x` casa só o nome exato do binário (não o updater nem o CLI).
    as_user sh -c "pkill -x schematize-gui" 2>/dev/null || true
  else
    warn "build da GUI Slint falhou — rode o install de novo. (Não instalamos GUI egui de fallback.)"
  fi

  # GUI do updater (janela amigável do gestor de atualizações) — OPCIONAL. Não depende do crate
  # `schematize` (fala só com o binário do updater), então sem `cargo update`. Best-effort: se
  # falhar, o app já está instalado — é só chrome. Não roda `die`.
  local ugui="$base/schematize-updater-gui"
  log "compilando a GUI do updater — schematize-updater-gui (opcional)"
  if _sync_repo "https://github.com/schematizeme/schematize-updater-gui.git" "$ugui" 2>/dev/null \
     && as_user sh -c "cd '$ugui' && cargo build --release" \
     && as_user install -m755 "$ugui/target/release/schematize-updater-gui" "$bin/schematize-updater-gui"; then
    ok "GUI do updater instalada (schematize-updater-gui)."
  else
    warn "GUI do updater não compilou (opcional) — segue sem ela."
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
