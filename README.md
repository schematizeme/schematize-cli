# schematize

Gerenciador do ecossistema **schematize** (Linux-first): instala e **versiona** as skills
do catálogo e roda o modo **overdev** — desenvolvimento contínuo à prova de parada
prematura, **sem travar pra perguntar**. É o "app no PC" que instala as skills e as tools
da casa; skills e futuras ferramentas entram pelo mesmo binário.

## Instalar

**Distros suportadas primariamente:** Debian 13, Linux Mint (base Debian/Ubuntu) e
openSUSE Leap. Outras: cai no binário estático.

**Um comando** (detecta a distro; usa `.deb` no apt, `.rpm` no zypper, ou binário; instala
deps e liga o agente):
```bash
curl -fsSL https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh | bash
```

**Compilar do fonte na máquina** (instala Rust via rustup + deps e compila):
```bash
curl -fsSL https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh | bash -s -- --from-source
```

**Baixar o pacote direto:**
```bash
# Debian / Mint / Ubuntu
curl -fLO https://github.com/schematizeme/schematize-cli/releases/latest/download/schematize_amd64.deb
sudo apt install ./schematize_amd64.deb
# openSUSE Leap
curl -fLO https://github.com/schematizeme/schematize-cli/releases/latest/download/schematize.x86_64.rpm
sudo zypper install --allow-unsigned-rpm ./schematize.x86_64.rpm
```

**Do fonte manualmente:**
```bash
git clone https://github.com/schematizeme/schematize-cli.git
cd schematize-cli && cargo install --path .
```

O pacote instala `/usr/bin/schematize` + o autostart do agente em `/etc/xdg/autostart/`
(inicia com a sessão, checa atualizações e notifica com botão **Atualizar**).

## Interface gráfica (GUI)

Janela de gerenciamento (egui) — lista skills (instalada vs latest) com botões de
atualizar, liga o agente e o overdev. Roda em **KDE** e **Cinnamon** (X11/Wayland).

**Um comando** (instala as libs gráficas, compila e cria o lançador no menu):
```bash
curl -fsSL https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh | bash -s -- --gui
```

**Manual** — instale as libs e compile com a feature `gui`:
```bash
# Debian/Mint:
sudo apt install build-essential pkg-config libx11-dev libxcursor-dev libxrandr-dev \
  libxi-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
# openSUSE Leap:
sudo zypper install -t pattern devel_basis && sudo zypper install pkg-config \
  libX11-devel libXcursor-devel libXrandr-devel libXi-devel libxkbcommon-devel \
  wayland-devel Mesa-libGL-devel libxcb-devel

git clone https://github.com/schematizeme/schematize-cli.git
cd schematize-cli && cargo install --path . --features gui   # instala schematize + schematize-gui
```
Abra com `schematize-gui` ou pelo menu de aplicativos. O CLI (`schematize`) e os pacotes
`.deb`/`.rpm` **não** exigem essas libs — só a GUI.

## Skills (instalação e versão)

```bash
schematize install --all        # instala todas as skills do catálogo
schematize install web go       # instala só algumas
schematize list                 # instalada vs última disponível
schematize update --all         # atualiza tudo pro latest
schematize remove node          # remove uma
```

Instala em `~/.claude/skills/<skill>/` e achata os comandos em `~/.claude/commands/`.
O estado de versões fica em `~/.claude/schematize/state.json`.

## Overdev — dev contínuo até o checklist fechar

A ideia: **põe pra rodar e sai** (comer/dormir/viver). O agente **não para** até o
checklist estar 100% e **não trava te perguntando** — parkeia a dúvida num txt e segue.

```bash
schematize overdev enable                 # registra os hooks no ~/.claude/settings.json (1x)
schematize overdev start "<objetivo>"     # ativa um run no projeto atual
# ... o agente preenche .overdev/CHECKLIST.md e trabalha ...
schematize overdev status                 # feitos / abertos / on-hold + perguntas parkeadas
schematize overdev stop                   # encerra o run
```

Como funciona:
- **Stop hook** (`overdev check`): rejeita a parada enquanto houver item `- [ ]` aberto ou o
  gate (`.overdev/gate.sh`) falhar. Item `- [~]` (on-hold) **não** bloqueia.
- **PreToolUse hook** (`overdev guard`, matcher `AskUserQuestion`): **veta** o pool
  bloqueante de perguntas em overdev — manda parkear e continuar.
- **Parkear pergunta:** `schematize overdev park "<item>" "<pergunta>"` registra em
  `./PERGUNTAS-OVERDEV.txt` (na base do projeto) e marca o item como `- [~]` (on-hold).
- **Guardrails:** teto de `--max` ciclos (default 200, anti-loop) e o gate de verificação
  (item só fecha com prova). **Inerte** fora de um run — seguro deixar habilitado.

Detalhe normativo do modo: skill `schematize-engineering`, `references/overdev.md` (`/eng-overdev`).

## Estados do checklist
`- [ ]` aberto (tem que fazer) · `- [x]` feito (verificado) · `- [~]` on-hold (pergunta
parkeada, não bloqueia o fim do run).

MIT.
