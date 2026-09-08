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
#   curl -fsSL .../install.sh | bash -s -- --deployer  # instala TAMBÉM o deployer (SSH/VPS)
#   curl -fsSL .../install.sh | bash -s -- --optimizer # instala TAMBÉM o optimizer (recursos)
set -euo pipefail

REPO="schematizeme/schematize-cli"
BASE="https://github.com/$REPO/releases/latest/download"
API="https://api.github.com/repos/$REPO/releases/latest"
MODE="source"   # padrão: compilar do fonte
for a in "$@"; do case "$a" in
  --from-source|--source) MODE=source;; --binary) MODE=binary;;
  --package|--deb|--rpm) MODE=package;; --auto) MODE=package;;
  --gui) : ;;  # compat: no-op
  --deployer) DEPLOYER=1;;   # instala TAMBEM o schematize-deployer (ver o bloco no install_source)
  --optimizer) OPTIMIZER=1;; # instala TAMBEM o schematize-optimizer (idem)
esac; done
: "${DEPLOYER:=0}"
: "${OPTIMIZER:=0}"

log() { printf '\033[1;36m▶ %s\033[0m\n' "$*"; }
ok()  { printf '\033[1;32m✓ %s\033[0m\n' "$*"; }
die() { printf '\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }
# `warn` era USADA em quatro pontos (remoção do .deb/.rpm antigo, falha ao baixar o
# updater) e NUNCA foi definida. Sob `set -euo pipefail` isso não é um aviso perdido: o
# nome não resolve, o shell sai 127 e o `set -e` ABORTA a instalação — a mensagem que
# deveria tranquilizar o usuário virava `install.sh: linha 285: warn: command not found`.
# Avisar não pode ser mais perigoso que o problema que se está avisando.
warn() { printf '\033[1;33m⚠ %s\033[0m\n' "$*" >&2; }

# ---------------------------------------------------------------------------
# AUTO-ATUALIZAÇÃO DESTE PRÓPRIO SCRIPT — antes de qualquer outra coisa.
#
# O one-liner do site aponta pra releases/latest/download/install.sh, que é um
# asset CONGELADO na última tag. Mas o que ele instala vem da MAIN. As duas
# coisas divergem sozinhas com o tempo, e foi exatamente assim que a instalação
# quebrou: um script publicado na v0.33 mandando `--features gui` a um crate da
# main onde essa feature já não existe (a GUI egui saiu do CLI).
#
# Consertar só o texto do script não resolve — o link do site continuaria
# servindo o velho até a próxima release. A correção é estrutural: buscar o
# install.sh da main e passar a bola pra ele. Assim o script que instala é
# SEMPRE o que casa com o fonte que ele clona, por mais antigo que seja o link
# que a pessoa usou.
#
# Escapes: SCHEMATIZE_INSTALL_NO_SELF=1 pra testar um script local sem que ele
# se troque; SCHEMATIZE_INSTALL_SELF corta o laço (o script novo não busca de
# novo). Sem rede pro raw, segue com este mesmo script — degradar é melhor que
# falhar, e quem está offline não ia clonar nada mesmo.
# ---------------------------------------------------------------------------
RAW_INSTALL="https://raw.githubusercontent.com/$REPO/main/install.sh"
if [ -z "${SCHEMATIZE_INSTALL_SELF:-}" ] && [ -z "${SCHEMATIZE_INSTALL_NO_SELF:-}" ]; then
  _novo="$(mktemp)"
  if curl -fsSL "$RAW_INSTALL" -o "$_novo" 2>/dev/null \
     && [ -s "$_novo" ] \
     && head -n1 "$_novo" | grep -q '^#!'; then
    export SCHEMATIZE_INSTALL_SELF=1
    exec bash "$_novo" "$@"
  fi
  rm -f "$_novo"
fi

[ "$(uname -s)" = "Linux" ] || die "só Linux por enquanto."
# ---------------------------------------------------------------------------
# ELEVACAO: como este script instala pacote de sistema.
#
# Root -> nada de prefixo. Nao-root com `sudo` -> `sudo`. Nao-root SEM `sudo` -> vazio,
# e a instalacao de pacotes vira um AVISO acionavel em vez de um erro cru.
#
# POR QUE A TERCEIRA LINHA EXISTE (§37.48 -- "prever macacos")
#
# Antes, `SUDO="sudo"` era posto sempre que o UID nao era 0, sem perguntar se o `sudo`
# EXISTE. Numa maquina sem ele -- container minimo, imagem slim, distro enxuta -- a
# primeira instalacao de pacote morria com `sudo: command not found`, uma mensagem do
# shell que nao diz o que fazer e nao menciona o instalador. Medido: `debian:stable-slim`
# com um usuario comum reproduz isso em segundos.
#
# Edge case que um leigo atinge e BUG do software, nao erro de quem rodou. Agora o
# script diz quais pacotes faltam e como instala-los, e SEGUE ate onde da -- em muitos
# casos ate o fim, porque quem ja tem as libs nao precisa de pacote nenhum.
# ---------------------------------------------------------------------------
SUDO=""
SEM_ELEVACAO=0
if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    SEM_ELEVACAO=1
  fi
fi

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

# shellcheck disable=SC1091  # /etc/os-release e do sistema, nao esta neste repo pra seguir.
. /etc/os-release 2>/dev/null || true
FAMILY="unknown"
case " ${ID:-} ${ID_LIKE:-} " in
  *" debian "*|*" ubuntu "*|*" linuxmint "*) FAMILY="debian" ;;
  *" suse "*|*" opensuse "*|*" sles "*|*" fedora "*|*" rhel "*) FAMILY="rpm" ;;
esac
log "distro: ${PRETTY_NAME:-desconhecida} (família: $FAMILY) — modo: $MODE"

pkg_install() {
  # Sem root e sem sudo nao ha como instalar pacote de sistema. Dizer isso -- com a lista
  # e o comando -- e util; deixar o shell responder `sudo: command not found` nao e.
  if [ "$SEM_ELEVACAO" = 1 ]; then
    warn "nao consigo instalar pacote do sistema: voce nao e root e nao ha \`sudo\` aqui."
    warn "  faltam: $*"
    warn "  instale como root e rode este script de novo, por exemplo:"
    case "$FAMILY" in
      debian) warn "    apt-get update && apt-get install -y $*" ;;
      rpm)    warn "    zypper install -y $*   (ou: dnf install -y $*)" ;;
      *)      warn "    use o gerenciador de pacotes da sua distro" ;;
    esac
    warn "  sigo em frente — se as bibliotecas ja estiverem na maquina, nada se perde."
    return 1
  fi
  case "$FAMILY" in
    debian) $SUDO apt-get update -qq && $SUDO apt-get install -y "$@" ;;
    rpm)    if command -v zypper >/dev/null; then $SUDO zypper --non-interactive install -y "$@"; else $SUDO dnf install -y "$@"; fi ;;
    *) log "instale manualmente: $*" ;;
  esac
}
ensure_runtime_deps() {
  local miss=(); for b in curl unzip git; do command -v "$b" >/dev/null || miss+=("$b"); done
  if [ ${#miss[@]} -gt 0 ]; then pkg_install "${miss[@]}" || true; fi
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
# ---------------------------------------------------------------------------
# O que JA esta instalado nao se instala de novo — e sobretudo nao se pede sudo pra isso.
#
# POR QUE EXISTE: o `gui_build_deps` chamava o gerenciador de pacotes SEMPRE, mesmo com
# tudo presente. Numa maquina sem sudo, ou sem tty pra digitar a senha (instalacao
# disparada de dentro de um script, sessao remota, CI), o `sudo` falha na hora e — como
# o `gui_build_deps` era chamado sem `|| true`, sob `set -e` — a instalacao do fonte
# MORRIA por causa de dependencia que ja estava instalada.
#
# Piso "prever macacos" (§37.48): nao quebre por invocacao nao prevista, e nunca peca
# privilegio pra nao fazer nada.
#
# POR QUE `rpm -q --whatprovides` E NAO `rpm -q`: no openSUSE `pkg-config` e
# `sqlite-devel` sao PROVIDES virtuais — quem os fornece sao `pkgconf-pkg-config` e
# `sqlite3-devel`. Com `rpm -q` puro os dois apareceriam como faltantes numa maquina que
# os tem, e o sudo seria pedido a toa exatamente no caso que esta funcao quer evitar.
# ---------------------------------------------------------------------------
pkgs_faltantes() { # <pkg...> — imprime, um por linha, so os que NAO estao presentes
  local p
  for p in "$@"; do
    case "$FAMILY" in
      debian) dpkg -s "$p" >/dev/null 2>&1 || printf '%s\n' "$p" ;;
      rpm)    rpm -q --whatprovides "$p" >/dev/null 2>&1 || printf '%s\n' "$p" ;;
      # Familia desconhecida: nao da pra afirmar que esta instalado, entao trata como
      # faltante e deixa o pkg_install decidir. Nao inventa VERDE sobre o que nao checou.
      *)      printf '%s\n' "$p" ;;
    esac
  done
}

# Instala so o que falta da lista; se nao falta nada, nao encosta no gerenciador.
instala_faltantes() { # <rotulo> <pkg...>
  local rotulo="$1"; shift
  local falta=()
  mapfile -t falta < <(pkgs_faltantes "$@")
  if [ ${#falta[@]} -eq 0 ]; then
    ok "$rotulo: ja presentes — nao vou pedir sudo pra nao fazer nada."
    return 0
  fi
  log "$rotulo: instalando o que falta (${falta[*]})"
  # SEM_ELEVACAO e degradacao ANUNCIADA (o `pkg_install` ja explicou e deu o comando):
  # nao pode derrubar o script sob `set -e`, senao a instalacao morre onde ela ainda
  # podia terminar. Qualquer OUTRA falha do gerenciador continua abortando -- ali o erro
  # e real e seguir em frente esconderia a causa.
  if [ "$SEM_ELEVACAO" = 1 ]; then
    pkg_install "${falta[@]}" || true
    return 0
  fi
  pkg_install "${falta[@]}"
}

gui_runtime_deps() {
  case "$FAMILY" in
    debian) instala_faltantes "libs de runtime da GUI" libx11-6 libxcursor1 libxrandr2 libxi6 \
              libxkbcommon0 libwayland-client0 libgl1 libfontconfig1 || true ;;
    rpm)    instala_faltantes "libs de runtime da GUI" libX11-6 libXcursor1 libXrandr2 libXi6 \
              libxkbcommon0 libwayland-client0 Mesa-libGL1 fontconfig || true ;;
  esac
}
# libs de BUILD — só para --from-source.
gui_build_deps() {
  case "$FAMILY" in
    # libfontconfig1-dev: o Slint 1.17 (fontique no núcleo) LINKA a libfontconfig no build.
    # libsqlite3-dev: PREFERIR a lib da distro a compilar o SQLite embutido (~250 mil
    # linhas de C a cada build limpo). Se ela estiver aqui, o build do CLI linka a dela.
    debian) instala_faltantes "libs de build da GUI" build-essential pkg-config libx11-dev \
              libxcursor-dev libxrandr-dev libxi-dev \
              libxkbcommon-dev libwayland-dev libgl1-mesa-dev libxcb1-dev libxcb-render0-dev \
              libxcb-shape0-dev libxcb-xfixes0-dev libfontconfig1-dev libsqlite3-dev ;;
    rpm)    instala_faltantes "libs de build da GUI" gcc gcc-c++ make pkg-config libX11-devel \
              libXcursor-devel libXrandr-devel \
              libXi-devel libxkbcommon-devel wayland-devel Mesa-libGL-devel libxcb-devel fontconfig-devel \
              sqlite-devel ;;
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

# O `install_updater` vivia aqui. Foi removido pelo ADR-0013 — quem o sucede é o
# `install_market`, mais abaixo, e a diferença nao e so de nome: aquele baixava um gestor
# que era so gestor; este baixa o programa que instala e atualiza todo o ecossistema.
# ---------------------------------------------------------------------------
# PURGA — mata QUALQUER instalação anterior antes de instalar a nova.
#
# Por que isto existe: o schematize já pôde ser instalado em QUATRO lugares —
# `/usr/bin` (pacote .deb/.rpm), `/usr/local/bin` (self-update via pkexec),
# `~/.local/bin` (fallback quando os outros não são graváveis) e `~/.cargo/bin`
# (fonte/updater). Quem instalou por caminhos diferentes ao longo do tempo fica com
# várias cópias, e quem "ganha" é quem estiver primeiro no PATH — que pode ser a
# MAIS VELHA. Foi assim que uma máquina "atualizada" voltou a rodar a v0.35: o
# binário novo entrou num diretório, e o PATH continuou resolvendo pro outro.
#
# Atualizar não resolve isso, porque o problema não é a versão — é a ambiguidade.
# Então: antes de instalar, não sobra nenhum vestígio de instalação anterior.
#
# O que NÃO é tocado: dependências do sistema (libs, rustup) e DADOS do usuário —
# `~/.claude` (skills, settings) e `~/.schematize` (config, overdev, cache de
# build). Purga instalação, não o trabalho de ninguém.
# ---------------------------------------------------------------------------
# Os binários da casa. A lista de PURGA inclui os nomes `overflow*` do interregno em
# que o app se chamou Overflow: uma cópia com aquele nome sobrevivendo num dir de
# maior precedência no PATH é exatamente o tipo de fantasma que faz o app "voltar"
# pra uma versão velha — o bug que esta purga existe pra matar.
#
# O `schematize-deployer` e o `schematize-optimizer` NÃO estão nesta lista, e a ausência é
# DELIBERADA. A purga roda em TODA instalação; se eles entrassem aqui, um `install.sh` sem
# `--deployer` — o caso normal — passaria a APAGAR o deployer de quem o tem. Instalar o app
# não pode desinstalar outro, pelo mesmo motivo que atualizar não pode instalar o que
# ninguém pediu.
#
# Eles também não precisam da purga: só são instalados em `~/.cargo/bin`, e o `install -m755`
# sobrescreve. A ambiguidade de "quatro lugares possíveis" que criou esta função nunca
# existiu para eles.
#
# Os nomes ANTIGOS `deployer`/`optimizer` (sem prefixo) são caso diferente, e estão na lista
# de APOSENTADOS abaixo — ver a nota lá.
#
# O `schematize-updater` SAIU DAQUI e entrou nos APOSENTADOS (ADR-0013), e a diferença entre
# as duas listas é exatamente o ponto:
#
#   * Aqui (BINS) estão os nomes VIVOS. A purga poupa o diretório de DESTINO, porque é lá
#     que a instalação nova vai escrever — apagar antes abriria uma janela em que um build
#     longo que falha deixa a máquina sem app.
#   * Lá (APOSENTADOS) estão os nomes MORTOS. Eles NÃO têm instalação nova para escrever por
#     cima, então são apagados INCLUSIVE no destino. Deixá-los cria um binário órfão no PATH.
#
# Trocar um pelo outro erra nos dois sentidos: nome vivo na lista de aposentados DESINSTALA
# o app de alguém; nome morto na lista de vivos é o que NÃO é apagado, e vira o fantasma.
BINS="overflow overflow-gui overflow-updater overflow-updater-gui \
schematize schematize-gui schematize-market-gui"

purge_previous() {
  log "removendo instalações anteriores (binário, pacote, lançador) — dados e deps ficam"

  # 1) Processos vivos: um binário em execução segura o inode e reabre a versão velha.
  for b in $BINS; do pkill -x "$b" 2>/dev/null || true; done

  # 2) Pacotes da distro. Enquanto o pacote existir, ele repõe /usr/bin no próximo
  #    `apt/zypper upgrade` — remover só o arquivo não bastaria.
  if command -v dpkg >/dev/null && dpkg -l schematize 2>/dev/null | grep -q '^ii'; then
    log "removendo o pacote .deb do schematize"
    $SUDO apt-get remove -y schematize >/dev/null 2>&1 || $SUDO dpkg -r schematize >/dev/null 2>&1 || \
      warn "não removi o .deb — rode: sudo apt remove schematize"
  fi
  if command -v rpm >/dev/null && rpm -q schematize >/dev/null 2>&1; then
    log "removendo o pacote .rpm do schematize"
    { command -v zypper >/dev/null && $SUDO zypper -n rm schematize; } >/dev/null 2>&1 \
      || $SUDO dnf -y remove schematize >/dev/null 2>&1 \
      || warn "não removi o .rpm — rode: sudo zypper rm schematize (ou dnf remove)"
  fi

  # 3) Binários soltos, em todo diretório que algum caminho de instalação já usou —
  #    MENOS o destino desta instalação ($1). Lá o binário é SUBSTITUÍDO no fim; apagar
  #    antes abriria uma janela em que um build longo que falha deixa a máquina sem app
  #    nenhum. As cópias que causam o bug são as OUTRAS: são elas que o PATH pega
  #    primeiro. Some com elas e a ambiguidade acaba, sem desarmar ninguém no caminho.
  # Binários APOSENTADOS — também no dir de DESTINO. A purga normal poupa o destino de
  # propósito: é lá que a instalação nova vai escrever, e apagar antes abriria uma janela
  # sem app. Mas estes nomes saíram de circulação e NÃO têm instalação nova pra escrever
  # por cima; deixá-los cria um binário órfão no PATH.
  #
  #   `overflow*`  — o interregno em que o app se chamou Overflow.
  #   `deployer`, `optimizer` — o nome sem prefixo, antes do ADR-0012. Hoje os binários são
  #   `schematize-deployer` e `schematize-optimizer`; o nome curto não é mais escrito por
  #   ninguém, então sobreviveria no PATH apontando para uma versão congelada — e o app,
  #   atualizado ao lado, pareceria "não ter mudado nada".
  #   `schematize-updater-gui` — a JANELA antes de o dono dela mudar (ADR-0014). O binário
  #   agora é `schematize-market-gui`; deixar o antigo cria duas entradas no menu, uma delas
  #   abrindo uma tela que fala com um gestor que não existe mais.
  #   `schematize-updater` — absorvido pelo `schematize-market` (ADR-0013). É o caso mais
  #   perigoso da lista, porque ele não fica só ocupando espaço: ele RESPONDE. Um
  #   `schematize-updater update` numa máquina onde o market já assumiu executa um gestor
  #   congelado, que reinstala pelo caminho antigo e desfaz o que o novo fez.
  #
  # Note a diferença para a nota do BINS: ali o nome está VIVO e apagá-lo desinstalaria o
  # app de alguém. Aqui o nome está MORTO, e não apagá-lo é que quebra.
  for d in "$TARGET_HOME/.cargo/bin" "$TARGET_HOME/.local/bin" /usr/local/bin /usr/bin; do
    for b in overflow overflow-gui overflow-updater overflow-updater-gui \
             deployer optimizer schematize-updater schematize-updater-gui; do
      [ -e "$d/$b" ] && { rm -f "$d/$b" 2>/dev/null || $SUDO rm -f "$d/$b" 2>/dev/null; } && \
        log "removido binário aposentado: $d/$b"
    done
  done

  local destino="${1:-}"
  for b in $BINS; do
    for d in "$TARGET_HOME/.cargo/bin" "$TARGET_HOME/.local/bin"; do
      [ "$d" = "$destino" ] && continue
      as_user rm -f "$d/$b" "$d/$b.novo" "$d/$b.old" 2>/dev/null || true
    done
    for d in /usr/local/bin /usr/bin; do
      [ "$d" = "$destino" ] && continue
      [ -e "$d/$b" ] && { $SUDO rm -f "$d/$b" 2>/dev/null || true; }
    done
  done

  # 4) Lançadores, autostart e ícones — senão o menu do sistema segue abrindo o que
  #    não existe mais (ou pior: uma cópia antiga que sobrou).
  $SUDO rm -f /usr/share/applications/schematize-gui.desktop \
              /etc/xdg/autostart/schematize-agent.desktop 2>/dev/null || true
  as_user rm -f "$TARGET_HOME/.local/share/applications/schematize-gui.desktop" \
                "$TARGET_HOME/.config/autostart/schematize-agent.desktop" 2>/dev/null || true
  $SUDO rm -f /usr/share/icons/hicolor/*/apps/schematize.png 2>/dev/null || true
  as_user rm -f "$TARGET_HOME"/.local/share/icons/hicolor/*/apps/schematize.png 2>/dev/null || true

  ok "instalação anterior removida — instalando do zero."
}

# ---------------------------------------------------------------------------
# install_market — o BOOTSTRAP do gestor (ADR-0013).
#
# O QUE: poe o `schematize-market` na maquina. Binario PRE-COMPILADO quando ha um
# pra esta plataforma; senao, compila do fonte. Devolve 0 se o binario ficou la.
#
# ONDE: `install_source`, antes de tudo que depende dele (as flags `--deployer` e
# `--optimizer` delegam ao market).
#
# POR QUE ESTE PASSO E O CORACAO DO SCRIPT AGORA
#
# O `schematize-updater` era o unico artefato publicado pre-compilado por SO, e este
# script o baixava pronto justamente por ele ser pequeno e estavel. O ADR-0013 passa
# o papel de gestor pro market; com ele vem a obrigacao. E o que faz este script
# voltar a ser SO um bootstrap: ele instala o gestor, o gestor instala o resto.
#
# POR QUE NAO SOBRESCREVE O QUE JA EXISTE: o download vem do ultimo RELEASE, que pode
# estar ATRAS do que a maquina tem — o market se reconstroi a cada `update`, e
# sobrescrever aqui o REBAIXARIA, desfazendo correcoes dele mesmo. Foi assim que uma
# correcao no proprio updater deixou de chegar. Aqui e bootstrap de quem nao tem.
#
# POR QUE A FALHA FALA (piso R3): sem o market nao ha caminho de atualizacao nenhum
# nem instalacao de linguagem. Um `>/dev/null 2>&1` aqui e como dois apps sumiram do
# menu em silencio quando `desktop --instalar` virou `--install`.
# ---------------------------------------------------------------------------
install_market() {
  local bin="$TARGET_HOME/.cargo/bin" dst asset os arch url
  dst="$bin/schematize-market"
  as_user mkdir -p "$bin"

  if [ -x "$dst" ]; then
    ok "schematize-market ja instalado ($("$dst" --version 2>/dev/null | head -1))"
    return 0
  fi

  # Nomes de asset IDENTICOS aos que `nucleo::plataforma::market_asset_name()` monta
  # e aos que o `release.yml` do market publica — os tres sao travados por teste la.
  os="$(uname -s)"; arch="$(uname -m)"
  case "$os/$arch" in
    Linux/x86_64)  asset="schematize-market-linux-x86_64" ;;
    Darwin/arm64)  asset="schematize-market-macos-arm64" ;;
    Darwin/x86_64) asset="schematize-market-macos-x86_64" ;;
    *) asset="" ;;
  esac

  if [ -n "$asset" ]; then
    url="https://github.com/schematizeme/schematize_market_rs/releases/latest/download/$asset"
    log "baixando o schematize-market pronto ($asset)"
    if as_user sh -c "curl -fsSL -o '$dst' '$url'" 2>/dev/null && [ -s "$dst" ]; then
      as_user chmod +x "$dst" 2>/dev/null || true
      # O binario TEM de executar aqui. Um asset da glibc errada baixa com HTTP 200 e
      # nao roda — instala-lo e chamar de sucesso seria o gestor mentindo na primeira
      # frase que diz. Se nao executa, apaga e cai pro fonte, que sempre funciona.
      if as_user "$dst" --version >/dev/null 2>&1; then
        ok "schematize-market instalado ($dst)"
        return 0
      fi
      warn "o binario baixado nao executa nesta maquina (glibc/arquitetura?) — compilando do fonte."
      as_user rm -f "$dst" 2>/dev/null || true
    else
      as_user rm -f "$dst" 2>/dev/null || true
      warn "nao consegui baixar o schematize-market pronto (rede? release ainda nao cortado?)."
      warn "  tentei: $url"
      warn "  seguindo pelo fonte — leva minutos a mais, o resultado e o mesmo."
    fi
  else
    log "sem binario pronto do market pra $os/$arch — compilando do fonte"
  fi

  install_market_do_fonte
}

# ---------------------------------------------------------------------------
# install_market_do_fonte — o caminho confiavel do bootstrap.
#
# O QUE: clona o repo do market e compila. ONDE: `install_market`, quando nao ha
# asset pra plataforma ou o baixado nao executa.
#
# POR QUE E FUNCAO SEPARADA: e o unico caminho que funciona em TODA plataforma, e o
# `install_market` precisa poder chama-lo de dois pontos diferentes sem duplicar o
# bloco. Duplicar aqui seria duplicar justamente o passo que nao pode divergir.
# ---------------------------------------------------------------------------
install_market_do_fonte() {
  local base="$TARGET_HOME/.schematize/src" bin="$TARGET_HOME/.cargo/bin"
  local tgt="$TARGET_HOME/.schematize/target" mkt="$base/schematize_market_rs"
  as_user mkdir -p "$base" "$bin" "$tgt"
  log "compilando o schematize-market do fonte"
  if _sync_repo "https://github.com/schematizeme/schematize_market_rs.git" "$mkt"      && as_user sh -c "cd '$mkt' && CARGO_TARGET_DIR='$tgt' cargo build --release"      && as_user install -m755 "$tgt/release/schematize-market" "$bin/schematize-market"; then
    ok "schematize-market compilado e instalado."
    return 0
  fi
  # Erro nunca engolido (piso 4 e risco R3): sem o market a pessoa fica sem gestor de
  # atualizacao E sem instalacao de linguagem. A mensagem diz o que se perdeu e como repetir.
  warn "o schematize-market NAO foi instalado. O schematize segue funcionando, mas:"
  # ATENCAO ao citar comando aqui: crase dentro de aspas DUPLAS e substituicao de
  # comando em bash — um aviso escrito com crase EXECUTARIA o que ele so queria citar.
  # (O shellcheck pegou isto ainda no gate; a linha rodava `schematize-market update`.)
  warn "  - nao ha caminho de atualizacao (era o 'schematize-market update');"
  warn "  - nao ha instalacao de linguagem (go, rust, node...);"
  warn "  - as flags --deployer e --optimizer nao tem quem os instale."
  warn "  repita a mao: git clone https://github.com/schematizeme/schematize_market_rs"
  warn "               cd schematize_market_rs && cargo build --release"
  warn "               install -m755 target/release/schematize-market ~/.cargo/bin/"
  return 1
}

# ---------------------------------------------------------------------------
# install_janela_do_gestor — a interface amigavel, por BINARIO PRONTO (ADR-0014 D5).
#
# O QUE: poe o `schematize-market-gui` na maquina, baixando o asset do release do market.
# ONDE: `install_source`, depois do gestor. Best-effort: nunca falha a instalacao.
#
# A VOLTA POR CIMA QUE ESTE BLOCO DEU, e por que ela e a decisao certa nas duas pontas
#
# Ate o ADR-0013 este script COMPILAVA a janela do updater do fonte, em toda instalacao. O
# updater foi aposentado, e a janela ficou falando com um binario que nao existe mais — entao
# o bloco saiu: nao se instala software que so pode falhar. O ADR-0014 (D4) decidiu que ela
# nao e descontinuada, vira a janela do market; o D5 a publica como asset do release dele. Com
# dono e com asset, ela volta — e volta MELHOR, porque deixou de compilar.
#
# POR QUE SO O BINARIO PRONTO, sem fallback para o fonte: ela e chrome. Quem nao tem o asset
# (plataforma fora da matriz, release ainda nao cortado) fica sem a janela e com TODO o resto
# funcionando — `schematize-market` no terminal faz tudo que ela faz. Gastar minutos de build
# num item opcional, numa instalacao que ja levou minutos, e o oposto de respeitar o tempo de
# quem instalou.
# ---------------------------------------------------------------------------
install_janela_do_gestor() {
  local bin="$TARGET_HOME/.cargo/bin" dst asset os arch url
  dst="$bin/schematize-market-gui"
  as_user mkdir -p "$bin"

  [ -x "$dst" ] && { ok "janela do gestor ja instalada."; return 0; }

  os="$(uname -s)"; arch="$(uname -m)"
  case "$os/$arch" in
    Linux/x86_64)  asset="schematize-market-gui-linux-x86_64" ;;
    Darwin/arm64)  asset="schematize-market-gui-macos-arm64" ;;
    Darwin/x86_64) asset="schematize-market-gui-macos-x86_64" ;;
    *) log "sem janela do gestor pronta pra $os/$arch — seguindo sem ela (o terminal faz tudo)."
       return 0 ;;
  esac

  url="https://github.com/schematizeme/schematize_market_rs/releases/latest/download/$asset"
  log "baixando a janela do gestor ($asset)"
  if as_user sh -c "curl -fsSL -o '$dst' '$url'" 2>/dev/null && [ -s "$dst" ]; then
    as_user chmod +x "$dst" 2>/dev/null || true
    ok "janela do gestor instalada ($dst)."
    return 0
  fi
  # Best-effort NAO E MUDO (piso 4): a pessoa fica sem a janela e precisa saber por que, e
  # que nada mais se perdeu com isso.
  as_user rm -f "$dst" 2>/dev/null || true
  warn "nao consegui baixar a janela do gestor (rede? release ainda nao cortado?)."
  warn "  tentei: $url"
  warn "  seguindo sem ela — o \`schematize-market\` no terminal faz tudo que ela faz."
  return 0
}

# ---------------------------------------------------------------------------
# delega_ao_market <app> — instala um app do ecossistema PELO GESTOR (ADR-0013).
#
# O QUE: chama `schematize-market install <app> -y`. Devolve 0 se o app ficou la.
#
# ONDE: as flags `--deployer` e `--optimizer` do `install_source`.
#
# POR QUE O SCRIPT DEIXOU DE COMPILAR ESSES APPS (D5)
#
# Este arquivo tinha ~900 linhas e sabia instalar cinco apps — um segundo gestor de
# pacotes escrito em bash, ao lado do de verdade. Dois caminhos pra mesma coisa e
# nenhum deles dono. O market compila do fonte com o mesmo checkout persistente e o
# mesmo target compartilhado; aqui basta pedir.
#
# COMPATIBILIDADE: `--deployer` e `--optimizer` seguem funcionando ponta a ponta —
# quem tem o comando na mao nao precisa saber que o dono mudou.
#
# POR QUE FALA QUANDO FALHA: best-effort nao e mudo (piso 4). Se o market nao esta la,
# a mensagem diz por que o app nao veio, em vez de a flag virar no-op silencioso.
# ---------------------------------------------------------------------------
# ---------------------------------------------------------------------------
# garantir_o_gestor — poe o market na maquina e no menu. Idempotente.
#
# O QUE: chama `install_market` (que ja sai cedo se ele existir) e registra o icone.
# Nunca falha o script: sem o gestor a pessoa perde atualizacao e instalacao de
# linguagem, mas o schematize que ela acabou de instalar continua funcionando (piso 10).
#
# ONDE: `install_source`, ANTES da delegacao das flags (que precisa do market), e
# `post_config`, que TODO caminho de instalacao atravessa.
#
# POR QUE NOS DOIS LUGARES, E POR QUE ISSO E O CONSERTO DE UM BUG REAL
#
# O `install_market` estava so no `install_source`. Os caminhos `--binary` e `package`
# nunca o chamavam — instalavam o app e deixavam a maquina SEM GESTOR, o que depois do
# ADR-0013 significa sem caminho de atualizacao nenhum. O antecessor (`install_updater`)
# nao tinha esse buraco porque era chamado do `post_config`, por onde todos passam.
#
# Chamar dos dois pontos e seguro porque a funcao e idempotente: no `install_source` ela
# faz o trabalho, e no `post_config` ela ve o binario la e retorna na hora.
# ---------------------------------------------------------------------------
garantir_o_gestor() {
  if install_market; then
    registrar_no_menu "$TARGET_HOME/.cargo/bin/schematize-market" "schematize-market"
    return 0
  fi
  return 1
}

delega_ao_market() {
  local app="$1" mkt="$TARGET_HOME/.cargo/bin/schematize-market"
  if [ ! -x "$mkt" ]; then
    warn "$app nao foi instalado: o schematize-market nao esta na maquina, e e ele quem instala."
    warn "  resolva o market primeiro (veja as mensagens acima) e depois rode:"
    warn "  schematize-market install $app"
    return 1
  fi
  log "instalando o $app pelo gestor — schematize-market install $app"
  if as_user "$mkt" install "$app" -y; then
    ok "$app instalado."
    return 0
  fi
  warn "o $app nao foi instalado. O schematize segue funcionando — o app e opcional."
  warn "  tente de novo: schematize-market install $app"
  return 1
}

# ---------------------------------------------------------------------------
# PATH nos rc de shell.
#
# O QUE: garante que <dir> esteja no PATH dos terminais FUTUROS, acrescentando um
# export aos rc do usuario. Idempotente, e SEMPRE em modo append.
#
# ONDE: `post_config` (dir = ~/.cargo/bin, o caminho do modo `source`, que e o
# padrao) e `install_binary` (dir = /usr/local/bin ou ~/.local/bin).
#
# POR QUE EXISTE: ate 2026-09-06 este script NAO escrevia rc nenhum -- `grep -c
# 'bashrc\|profile\|zshrc' install.sh` dava 0. Ele dependia de um EFEITO COLATERAL
# do rustup, que o `ensure_rust` so executa quando `cargo` NAO existe. No openSUSE
# `zypper install rust` poe o cargo em /usr/bin, entao o rustup nunca roda, ninguem
# escreve o rc, e ~/.cargo/bin -- onde o build do fonte deixa o binario -- fica fora
# do PATH. O terminal grafico abre shell NAO-login (le ~/.bashrc, nao le ~/.profile),
# e o `schematize` "nao existe", enquanto o `post_config` imprime "pronto. Proximos
# passos:" mandando digitar comandos que nao resolvem.
#
# O defeito de fundo nao era o openSUSE: era depender de efeito colateral de
# ferramenta de terceiro, executado condicionalmente, sem nunca verificar o
# resultado. Agora o instalador faz ele mesmo, e confere.
#
# POR QUE APPEND E NUNCA LER-MODIFICAR-ESCREVER: o mesmo motivo documentado em
# `schematize_updater_rs::platform::acrescenta_path_no_rc`. Reescrever o arquivo
# inteiro a partir de uma leitura que pode falhar ja apagou `.bashrc` de gente
# (um acento em Latin-1 bastava). Em append o pior caso e uma linha duplicada.
#
# POR QUE NAO CHAMAR O BINARIO DO UPDATER PRA FAZER ISSO: o `ensure_path_setup` do
# updater roda dentro do `install.rs` DELE. Neste ponto do script o updater pode nem
# estar instalado (o `garantir_o_gestor` roda depois, e e best-effort). O PATH nao pode
# depender de um passo que tem permissao de falhar.
# ---------------------------------------------------------------------------
ensure_path_rc() {
  local dir="$1" rc f escreveu=0 ja=0
  # Dirs que todo PATH ja tem: escrever export pra eles e ruido, nao correcao.
  case "$dir" in /usr/bin|/bin|/usr/local/bin|/usr/sbin|/sbin) return 0 ;; esac

  for rc in .bashrc .profile .zshrc; do
    f="$TARGET_HOME/$rc"
    if [ ! -e "$f" ]; then
      # .zshrc so se JA existir: criar config de zsh pra quem nao usa zsh e sujeira.
      # .bashrc e .profile sao padrao e podem nascer aqui -- e no openSUSE o .bashrc
      # SEMPRE existe, entao este ramo e a excecao, nao a regra.
      [ "$rc" = ".zshrc" ] && continue
    elif [ ! -r "$f" ]; then
      # Existe e nao da pra ler: nao da pra saber se ja esta la. Nao escreve as cegas.
      warn "$f existe mas nao consigo ler -- deixei intacto. Acrescente a mao:"
      warn "  export PATH=\"$dir:\$PATH\""
      continue
    elif grep -qF -- "$dir" "$f" 2>/dev/null; then
      ja=1; continue   # ja esta la (nosso ou posto a mao) -- idempotente
    fi
    # Args por posicional, NUNCA interpolados no texto do script: um HOME com aspa
    # simples (`/home/o'brien`) quebraria a citacao e viraria execucao arbitraria.
    # O `$PATH` TEM de chegar literal no rc: expandir aqui congelaria, dentro do
    # arquivo do usuario, o PATH que existia no momento da instalacao. SC2016 esta
    # certo sobre o fato e errado sobre a intencao -- por isso a excecao e deste
    # comando, nao do arquivo (`-e SC2016` no CI apagaria a regra em silencio).
    # shellcheck disable=SC2016
    if as_user sh -c \
      'printf "\n# schematize: dir de instalacao no PATH\nexport PATH=\"%s:\$PATH\"\n" "$1" >> "$2"' \
      _ "$dir" "$f" 2>/dev/null; then
      escreveu=1
    else
      warn "nao consegui escrever em $f. Acrescente a mao:"
      warn "  export PATH=\"$dir:\$PATH\""
    fi
  done

  # VERIFICA em vez de supor (o defeito de fundo era justamente nao verificar).
  # Veredito por leitura do arquivo, nao por "o comando nao deu erro".
  if [ "$escreveu" = 1 ]; then
    if grep -qF -- "$dir" "$TARGET_HOME/.bashrc" 2>/dev/null \
       || grep -qF -- "$dir" "$TARGET_HOME/.profile" 2>/dev/null; then
      ok "$dir acrescentado ao PATH (.bashrc/.profile) -- vale no PROXIMO terminal."
    else
      warn "escrevi no rc mas nao consegui confirmar a linha. Verifique o PATH a mao."
    fi
  elif [ "$ja" = 1 ]; then
    ok "$dir ja estava no PATH dos seus rc."
  fi
  return 0
}

post_config() {
  local BIN="$TARGET_HOME/.cargo/bin/schematize"
  # A remoção de pacote/binário antigo agora é da `purge_previous`, que roda ANTES de
  # instalar qualquer coisa. Este bloco fica como rede: se alguém reinstalou o pacote
  # no meio do caminho, ele ainda conflita com a fonte.
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
  # O PATH tem de ser garantido ANTES do "próximos passos" abaixo, senão o script
  # imprime comandos que o terminal do usuário não resolve — o bug do openSUSE.
  ensure_path_rc "$TARGET_HOME/.cargo/bin"
  as_user "$BIN" autostart enable || true
  # TODO caminho de instalacao passa por aqui — inclusive `--binary` e `package`, que nao
  # entram no `install_source`. E o que garante que ninguem termine sem gestor.
  garantir_o_gestor || true
  echo; ok "pronto. Próximos passos:"
  # HONESTIDADE: se o dir ainda não está no PATH DESTA sessão, os comandos abaixo só
  # funcionam num terminal novo. Mandar digitá-los sem avisar é o que fez a pessoa
  # concluir que a instalação falhou, quando o que faltava era reabrir o terminal.
  case ":$PATH:" in
    *":$TARGET_HOME/.cargo/bin:"*) : ;;
    *) echo "    (abra um terminal NOVO — ou rode: export PATH=\"$TARGET_HOME/.cargo/bin:\$PATH\")" ;;
  esac
  echo "    schematize skills install --all   # instala as skills"
  echo "    schematize overdev enable         # liga o modo overdev"
  echo "    schematize gui                    # abre a janela (ou use o menu de apps)"
  echo "    schematize skills list            # versões instaladas vs latest"
}

install_binary() {
  purge_previous "$TARGET_HOME/.cargo/bin"
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
  # Antes isto era só um aviso ("⚠ adicione ao PATH") — pedir pro usuário fazer o que
  # o instalador pode fazer sozinho é o oposto do piso "prever macacos" (§37.48).
  ensure_path_rc "$dst"
  post_config
}
install_deb() {
  purge_previous /usr/bin
  ensure_runtime_deps; ensure_fonts
  local DL; DL="$(resolve_dl)"
  local t; t="$(mktemp --suffix=.deb)"; log "baixando .deb (CLI + GUI)"; curl -fSL -o "$t" "$DL/schematize_amd64.deb"
  chmod 644 "$t"   # deixa o _apt (sandbox de download) ler o arquivo — sem o aviso de permissão
  log "instalando via apt (resolve libs da GUI)"; $SUDO apt-get install -y "$t" || { $SUDO dpkg -i "$t"; $SUDO apt-get -f install -y; }
  rm -f "$t"; post_config
}
install_rpm() {
  purge_previous /usr/bin
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
  purge_previous "$TARGET_HOME/.cargo/bin"
  # Checkouts PERSISTENTES no HOME do usuário: `target/` cacheado → `cargo build` incremental (só o
  # que mudou recompila; deps pesadas tipo Slint não). Sem `cargo install --force` (que zerava o cache).
  local base="$TARGET_HOME/.schematize/src"; as_user mkdir -p "$base"
  local cli="$base/schematize-cli" gui="$base/schematize_gui_slint"
  local bin="$TARGET_HOME/.cargo/bin"; as_user mkdir -p "$bin"

  # TARGET COMPARTILHADO pelos repos que este script compila (CLI, GUI Slint, market).
  # 226 das dependências são as MESMAS em todos; com um `target/` por checkout elas
  # compilavam uma vez POR REPO (e ocupavam o disco uma vez por repo). Com um só, compilam
  # uma vez. Exige perfil de release idêntico — está documentado no Cargo.toml de cada um.
  local tgt="$TARGET_HOME/.schematize/target"; as_user mkdir -p "$tgt"
  export CARGO_TARGET_DIR="$tgt"

  # SQLite: se a distro tem a lib de desenvolvimento, LINKA a dela em vez de
  # compilar ~250 mil linhas de C a cada build limpo. (Isso vale só pra biblioteca
  # C — crate de Rust não dá pra reusar da distro: Rust não tem ABI estável.)
  local feats=""
  if pkg-config --exists sqlite3 2>/dev/null; then
    log "usando a libsqlite3 da distro (não compila o SQLite embutido)"
    feats="--no-default-features --features sqlite-do-sistema"
  fi

  # CLI SEM a feature `gui` — NÃO produz o schematize-gui egui (a única GUI é o Slint, repo próprio).
  log "compilando o CLI do fonte (incremental — recompila só o que mudou; 1ª vez leva minutos)"
  _sync_repo "https://github.com/$REPO.git" "$cli" || die "clone do CLI falhou"
  if ! as_user sh -c "cd '$cli' && CARGO_TARGET_DIR='$tgt' cargo build --release $feats"; then
    # §37.48: mensagem acionavel, sem culpa. "build do CLI falhou" nao diz nada a quem nao
    # le Rust — e quando a causa e a que este script JA ANUNCIOU (nao consegui instalar
    # pacote porque nao ha root nem sudo), nao dize-lo e deixar a pessoa procurar o que ja
    # sabemos. Medido em `debian:stable-slim` com usuario comum: o build morre em
    # `linker \`cc\` not found`, que e exatamente o `build-essential` que nao pode ser instalado.
    if [ "$SEM_ELEVACAO" = 1 ]; then
      warn "o build falhou, e a causa mais provavel e a de cima: faltam as ferramentas de"
      warn "compilacao (compilador C, pkg-config, libs de dev) que eu nao pude instalar"
      warn "porque voce nao e root e nao ha \`sudo\` nesta maquina."
      warn "  caminho mais curto: instale-as como root e rode este script de novo."
      warn "  ou, se houver binario pronto pra sua plataforma:  bash install.sh --binary"
    fi
    die "build do CLI falhou"
  fi
  as_user install -m755 "$tgt/release/schematize" "$bin/schematize"

  # GUI = Slint (a ÚNICA GUI). Se o build falhar, NÃO cai pro egui — melhor sem GUI que o fantasma.
  # A GUI depende do crate `schematize` como git-dep (branch=main); o Cargo.lock commitado FIXA um
  # commit e `git reset --hard` o restaura a cada update, então a versão embutida (`app_version()`)
  # ficava travada numa release velha. `cargo update -p schematize` avança o git-dep pro HEAD ANTES
  # de compilar (best-effort: offline segue com o lock). Sem isso, "atualizei mas abre versão antiga".
  log "compilando a GUI Slint — schematize-gui (incremental)"
  if _sync_repo "https://github.com/schematizeme/schematize_gui_slint.git" "$gui" 2>/dev/null \
     && { as_user sh -c "cd '$gui' && CARGO_TARGET_DIR='$tgt' cargo update -p schematize" 2>/dev/null || true; } \
     && as_user sh -c "cd '$gui' && CARGO_TARGET_DIR='$tgt' cargo build --release $feats" \
     && as_user install -m755 "$tgt/release/schematize-gui" "$bin/schematize-gui"; then
    ok "GUI Slint instalada (schematize-gui)."
    # Encerra GUI antiga ainda aberta — fechar a janela não matava o processo, e o relaunch reusava
    # a versão anterior. `pkill -x` casa só o nome exato do binário (não o updater nem o CLI).
    as_user sh -c "pkill -x schematize-gui; pkill -x overflow-gui" 2>/dev/null || true
  else
    warn "build da GUI Slint falhou — rode o install de novo. (Não instalamos GUI egui de fallback.)"
  fi

  # O `schematize-updater` NAO e mais compilado nem baixado aqui (ADR-0013).
  #
  # Ele era compilado neste ponto, do fonte, "junto com o app". Deixou de existir como app
  # separado: o `schematize-market` — instalado logo abaixo, por binario pronto quando ha —
  # e o unico responsavel por instalar e atualizar o ecossistema.
  #
  # Quem ja tem o binario antigo na maquina nao fica com ele: o nome esta na lista de
  # APOSENTADOS da `purge_previous`, que o apaga INCLUSIVE no diretorio de destino, dizendo
  # o que removeu. Ver a nota das duas listas la em cima.

  install_janela_do_gestor

# -----------------------------------------------------------------------------
# registrar_no_menu <caminho-do-binario> <nome>
#
# Cada app do ecossistema registra a PRÓPRIA entrada no menu de aplicativos — é o que o
# torna abrível sem o schematize (ADR-0010/0011).
#
# POR QUE ISTO VIROU FUNÇÃO, E POR QUE ELA FALA QUANDO FALHA
#
# Antes, os dois blocos chamavam `desktop --instalar >/dev/null 2>&1` inline. Quando a CLI
# dos apps foi traduzida para inglês, a flag virou `--install` — e as duas chamadas passaram
# a falhar **em silêncio**, porque o `>/dev/null` engolia o erro e o ramo `else` só dizia
# "instalado", sem mencionar o ícone. O resultado foi um app instalado que não aparecia em
# lista de software nenhuma, sem uma linha de saída que apontasse a causa.
#
# Best-effort continua certo: sem ícone é chato, derrubar a instalação por causa dele é
# pior. Mas best-effort **não é mudo**. Se o passo falhar, isto diz qual comando falhar e
# como consertar à mão — o §37.48 cobra mensagem acionável, não um sucesso que mente.
# -----------------------------------------------------------------------------
registrar_no_menu() {
  local exe="$1" nome="$2" saida
  if saida="$(as_user "$exe" desktop --install 2>&1)"; then
    ok "$nome já aparece no menu de aplicativos."
    return 0
  fi
  warn "$nome foi instalado, mas não consegui pôr o ícone no menu."
  warn "  o que falhou: $exe desktop --install"
  [ -n "$saida" ] && warn "  disse: $(printf '%s' "$saida" | head -n 2 | tr '\n' ' ')"
  warn "  rode o comando acima para tentar de novo; o app funciona pelo terminal do mesmo jeito."
  return 0
}

  # ---------------------------------------------------------------------------
  # MARKET — o GESTOR (ADR-0012 + ADR-0013). Vai SEMPRE, sem flag.
  #
  # POR QUE ESTE E DIFERENTE DE TODO O RESTO: ele nao e mais "mais um app". Depois do
  # ADR-0013 e ele quem instala e atualiza o ecossistema inteiro. Sem ele a maquina fica
  # sem caminho de atualizacao E sem instalacao de linguagem — por isso o passo tenta o
  # binario pronto primeiro (segundos) e so entao o fonte (minutos), em vez de compilar
  # sempre como fazia antes.
  #
  # Segue best-effort quanto ao SCHEMATIZE (piso 10): se o market nao vier, o app ja
  # esta instalado e funcionando. Mas a falha grita — ver `install_market_do_fonte`.
  # ---------------------------------------------------------------------------
  if garantir_o_gestor; then
    ok "schematize-market instalado. Veja tudo com: schematize-market list"
  fi

  # ---------------------------------------------------------------------------
  # DEPLOYER e OPTIMIZER — OPT-IN, com `--deployer` / `--optimizer`.
  #
  # POR QUE NAO ENTRAM POR PADRAO: um guarda credencial, o outro MEXE NA MAQUINA (slice
  # de systemd e, adiante, cmdline de kernel). Instalar em quem nao pediu e o oposto do
  # piso 10 e do §37.48.
  #
  # POR QUE O SCRIPT NAO OS COMPILA MAIS (D5): os dois blocos que estavam aqui clonavam,
  # compilavam e instalavam — exatamente o que o `schematize-market install` faz, com o
  # mesmo checkout persistente e o mesmo target compartilhado. Eram ~40 linhas de bash
  # duplicando um gestor de pacotes de verdade. Agora este script so pede.
  #
  # As flags seguem valendo ponta a ponta: a compatibilidade e com quem ja tem o comando
  # na mao, nao com o jeito antigo de cumpri-lo.
  # ---------------------------------------------------------------------------
  # `if` e nao `A && B || C`: aqui a delegacao PODE falhar (o market pode nao estar la),
  # e a falha ja fala por si — o `|| true` faria o `set -e` engolir o codigo de saida
  # sem que ninguem lesse a diferenca. Ver SC2015.
  if [ "$DEPLOYER" = 1 ]; then
    delega_ao_market schematize-deployer || true
  fi
  if [ "$OPTIMIZER" = 1 ]; then
    delega_ao_market schematize-optimizer || true
  fi

  # Os `target/` por-repo de antes do target compartilhado não são mais lidos por
  # build nenhum — viram dezenas de GB de lixo. O script mudou o layout, o script
  # limpa: pedir pro usuário apagar à mão é o oposto do piso da casa. Só DEPOIS dos
  # builds (se algum falhar, o cache antigo continua lá) e só nos caminhos que este
  # script criou — nada de varrer por padrão.
  #
  # A lista e por CAMINHO, nao por variavel de bloco: o `$ugui` saiu junto com o bloco que
  # compilava a janela do updater (ADR-0013), e o `shellcheck` pegou a referencia orfa que
  # sobrou aqui — SC2154, "referenced but not assigned". Sob `set -u` isso e um abort. Os
  # checkouts antigos daqueles repos continuam listados de proposito: eles EXISTEM na
  # maquina de quem instalou antes, e sao justamente o lixo que este bloco existe pra levar.
  local liberado=0 mb
  for antigo in "$cli/target" "$gui/target" \
                "$base/schematize-updater-gui/target" \
                "$base/schematize-updater/target" \
                "$base/schematize_market_rs/target"; do
    [ -d "$antigo" ] || continue
    mb="$(du -sm "$antigo" 2>/dev/null | cut -f1)"; mb="${mb:-0}"
    as_user rm -rf "$antigo" 2>/dev/null || true
    liberado=$(( liberado + mb ))
  done
  if [ "$liberado" -gt 0 ]; then
    ok "liberados ${liberado} MB de target/ antigo (agora há um só, compartilhado)."
  fi

  install_gui_launcher
  post_config
}

# ---------------------------------------------------------------------------
# SCHEMATIZE_INSTALL_LIB=1 -> define as funcoes e PARA, sem instalar nada.
#
# Existe pra que `tests/install_path_rc.rs` exercite a `ensure_path_rc` DE VERDADE,
# em vez de reimplementar a logica no teste (teste que reimplementa nao prova o
# original) ou de recortar a funcao por numero de linha (allowlist ancorada em linha
# e armadilha -- o dia em que erra e o dia em que isenta a linha errada).
#
# Irmao do `SCHEMATIZE_INSTALL_NO_SELF`, que ja existia com o mesmo espirito.
# ---------------------------------------------------------------------------
if [ -n "${SCHEMATIZE_INSTALL_LIB:-}" ]; then
  # `return` so e valido se o script foi SOURCEADO; se foi EXECUTADO, e erro. Em vez de
  # tentar e cair no fallback (o que faz o shellcheck ver o `exit` como inalcancavel),
  # PERGUNTA: em bash, `BASH_SOURCE[0]` difere de `$0` exatamente quando ha source.
  if [ "${BASH_SOURCE[0]}" != "$0" ]; then
    return 0
  fi
  exit 0
fi

case "$MODE" in
  source) install_source ;;
  binary) install_binary ;;
  package)
    # O `A && B || C` abaixo NAO e if-then-else, e nao quer ser: se o pacote da
    # distro nao existe OU a instalacao dele falha, o fallback pro binario e o
    # comportamento pretendido. SC2015 alerta sobre o fato, que aqui e a intencao.
    # shellcheck disable=SC2015
    case "$FAMILY" in
      debian) command -v apt-get >/dev/null && install_deb || install_binary ;;
      rpm)    (command -v zypper >/dev/null || command -v dnf >/dev/null) && install_rpm || install_binary ;;
      *)      install_binary ;;
    esac ;;
  *) install_source ;;
esac
