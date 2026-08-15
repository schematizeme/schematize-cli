# schematize

Gerenciador do ecossistema **schematize** (Linux-first, **multi-idioma**): instala e
**versiona** as skills do catálogo, roda o modo **overdev** (dev contínuo à prova de
parada, **sem travar pra perguntar**), **diagnostica** o ambiente, **atualiza a si
mesmo** e traz as **novidades do blog** ([blog.schematize.net](https://blog.schematize.net)).
É o "app no PC" da casa; skills e futuras ferramentas entram pelo mesmo binário.

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

**Já vem pronta.** O `schematize-gui` é compilado no CI e entregue **dentro** do
`.deb`/`.rpm` (e como binário pré-compilado). O instalador normal já instala a janela
e cria o lançador no menu — **sem compilar, sem libs de dev**. As libs de runtime
(X11/GL/Wayland) o apt/zypper resolvem, e num desktop KDE/Cinnamon já estão lá.

```bash
curl -fsSL https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh | bash
schematize-gui   # ou procure "schematize" no menu de aplicativos
```

**Compilar do fonte** (só se quiser; aí sim instala as libs de build):
```bash
curl -fsSL https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh | bash -s -- --from-source
```
O CLI (`schematize`) não depende de nada gráfico; a GUI é um binário separado
(`schematize-gui`, feature `gui`).

## Mais que skills

```bash
schematize status              # painel: versões, agente, overdev, idioma, links
schematize doctor [--fix]      # diagnostica o ambiente e conserta o que dá
schematize upgrade [--force]   # atualiza o próprio schematize pro latest
schematize news                # últimos posts de blog.schematize.net
schematize blog                # abre o blog no navegador
schematize open site|blog|github
```

## Idiomas (i18n)

A interface (CLI + GUI) é multi-idioma. Por padrão detecta o idioma do sistema
(`$LANG`), com fallback pro inglês. 11 idiomas inclusos: **en, es, it, fr, de, pt,
ja, zh, ru, ar, hi**.

```bash
schematize lang            # mostra o idioma atual
schematize lang --list     # lista os idiomas disponíveis
schematize lang pt         # define o idioma (persistente)
```

Novo idioma = soltar um `src/i18n/<code>.json` e uma linha em `i18n.rs`.

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

## Painel auxiliar + grafo (fora do CLI/VSCode)

O overdev começa por uma **Fase 0** (colher as decisões acordadas → carregar o grafo do
index → planejamento pesado → só então tickar). O `schematize` **guarda esse contexto** e o
mostra num **painel HTML no browser** — para acompanhar o run enquanto o agente trabalha.

```bash
schematize panel                 # gera um HTML self-contained e abre no navegador
schematize graph obsidian        # exporta o index como vault Obsidian (markdown + [[wikilinks]])
schematize graph obsidian --out ~/vault
```

- **`schematize panel`** lê `.overdev/*` (objetivo, checklist com feitos/abertos/on-hold,
  `DECISOES.md`, `PLAN.md`, perguntas parkeadas) **e** o grafo do index
  (`<projeto>_archive/index/`), e renderiza uma **tela de grafos force-directed estilo
  Obsidian** — cada nó **linkado ao `arquivo:linha`** (abre no editor via `vscode://`). Sem
  CDN, um arquivo só. Também acessível pelo botão **"Abrir painel"** da GUI.
- **`schematize graph obsidian`** transforma o index num **vault Obsidian** navegável no
  Graph View (uma nota por função/serviço, com `[[wikilinks]]` de quem-chama-quem).
- O painel é **auxiliar e read-mostly**: o juiz do "terminou" segue sendo o checklist + gate.

## Estados do checklist
`- [ ]` aberto (tem que fazer) · `- [x]` feito (verificado) · `- [~]` on-hold (pergunta
parkeada, não bloqueia o fim do run).

MIT.
