# schematize

Gerenciador do ecossistema **schematize** (Linux-first, **multi-idioma**): instala e
**versiona** as skills do catálogo, roda o modo **overdev** (dev contínuo à prova de
parada, **sem travar pra perguntar**), **diagnostica** o ambiente, **atualiza a si
mesmo** e traz as **novidades do blog** ([blog.schematize.net](https://blog.schematize.net)).
É o "app no PC" da casa; skills e futuras ferramentas entram pelo mesmo binário.

## Instalar

### Kit completo (recomendado)

Hub + market (sempre na versão latest) + todos os apps e janelas + skills + `schematize doctor`,
num comando. Cada passo que falha é reportado sem derrubar os outros; o resumo final diz o que
faltou e o código de saída é diferente de zero se algo falhou:

```bash
curl -fsSL https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh | bash -s -- --completo
```

**FreeBSD (amd64):** o `bash` não vem na base, então o one-liner usa o `sh` e o `fetch` da base.
O instalador instala o bash por `pkg` (root, `sudo` ou `doas`), baixa o binário
`schematize-freebsd-x86_64` e instala em `~/.cargo/bin` sem sudo:

```sh
fetch -o - https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh | sh -s -- --completo
```

(`--all` e `--kit` são sinônimos de `--completo`.) O `schematize-market` é instalado **sempre**,
mesmo sem flag, e o instalador **troca** um market antigo pela latest sem depender do
self-update dele. Se `~/.cargo/bin` não estiver no seu PATH, o fim da instalação mostra o
comando exato pro seu shell.

### Só o hub

**Padrão: binário pré-compilado** do release mais novo — sem Rust, sem compilador, sem libs
de build (ADR-0022). Se não houver binário pra sua plataforma (ou o release ainda não saiu), o
instalador avisa com clareza e **compila do fonte por você**. Compilar na máquina de propósito
é opcional: `--from-source`.

```bash
curl -fsSL https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh | bash
```

**Distros com deps automáticas:** Debian 13 / Mint / Ubuntu e openSUSE Leap / Fedora. Outras:
instale à mão as libs de X11/Wayland/GL `-dev` da sua distro e rode o mesmo comando.

**Opcionais:**
```bash
# compilar na sua máquina (instala Rust e as libs de build da GUI; pede sudo; leva minutos)
curl -fsSL https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh | bash -s -- --from-source
# pacote .deb/.rpm da distro
curl -fsSL https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh | bash -s -- --package
```
(`--binary` continua aceita: é o mesmo que o padrão.)

**Do fonte manualmente:**
```bash
# CLI
git clone https://github.com/schematizeme/schematize-cli.git
cd schematize-cli && cargo install --path .

# GUI (repo SEPARADO — consome o CLI como git-dep)
git clone https://github.com/schematizeme/schematize_gui_slint.git
cd schematize_gui_slint
cargo update -p schematize     # a ordem importa: sem isso a janela abre a versão VELHA
cargo install --path .
```

O instalador faz os dois de uma vez. Instala `schematize` + `schematize-gui` em
`~/.cargo/bin` e liga o autostart do agente
(inicia com a sessão, checa atualizações e notifica). **`schematize upgrade`** delega ao
`schematize-market` (binário do release); `--from-source` do instalador é o caminho de fonte.

## Interface gráfica (GUI)

O **hub onde se faz a schematização** — janela egui com **abas**, tudo dentro do app
(nada de aba de navegador que some quando fecha):

- **Skills** — gestor: **checkbox por skill**, **Instalar/atualizar selecionados** e **Remover
  selecionados** em **massa e em paralelo**, seleção rápida (Tudo/Pendentes/Nenhum), coluna
  Estado, e o próprio CLI na lista. O **botão de atualizar nunca fica no-op** (pensado pra quem
  não é dev): binário + **pkexec** (senha gráfica, sem terminal) e, se faltar binário/falhar,
  **abre um terminal** com o rebuild do fonte; depois pede pra **reiniciar a janela**. A versão
  é lida do **fonte (raw)**, não da API do GitHub (60/h) — diagnóstico em `schematize debug`.
- **Overdev** — o run do projeto **nativo na janela**: objetivo, progresso (feitos/abertos/
  on-hold), o **checklist** colorido por estado, e Decisões/Plano/Perguntas parkeadas.
- **Grafo** — a **tela de grafos do index** (force-directed **estilo Obsidian**) desenhada
  **dentro do app**: arrasta, dá zoom, busca, clica um nó pra ver o `arquivo:linha` e **abrir
  no editor**; botões **Exportar Obsidian** e **abrir no navegador** (o HTML fica como extra,
  não como o único jeito de ver).

Você **escolhe o projeto** num seletor (recentes lembrados + colar caminho + pasta atual), e
o Overdev/Grafo mostram aquele projeto — persistente, sempre ali. Roda em **KDE** e
**Cinnamon** (X11/Wayland).

**A GUI compila junto com o CLI** no install padrão — ela é um **repo à parte**
(`schematize_gui_slint`, Slint) que consome o crate `schematize` como git-dep, e o
instalador clona, roda `cargo update -p schematize`, compila e cria o lançador no menu.
Puxa também as **libs de build** (X11/Wayland/GL `-dev`).
Depois é só:
```bash
schematize-gui   # ou procure "schematize" no menu de aplicativos
```
O CLI (`schematize`) não depende de nada gráfico; a GUI é um binário separado
(`schematize-gui`, feature `gui`). Quem só quer o CLI pode compilar sem a feature `gui`.

## Mais que skills

```bash
schematize status              # painel: versões, agente, overdev, idioma, links
schematize doctor [--fix]      # diagnostica o ambiente e conserta o que dá
schematize debug               # diagnóstico do atualizador/versão (rate limit, exe, catálogo, log)
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

Skills são uma FUNCIONALIDADE do app, agrupadas sob `schematize skills`:

```bash
schematize skills install --all   # instala todas as skills do catálogo
schematize skills install web go  # instala só algumas
schematize skills list            # instalada vs última disponível
schematize skills update --all    # atualiza tudo pro latest
schematize skills remove node     # remove uma
```

Os antigos `schematize install|update|list|remove` seguem válidos como aliases (compat).
Para atualizar o PRÓPRIO app (o binário), é outra coisa: `schematize upgrade`.

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
