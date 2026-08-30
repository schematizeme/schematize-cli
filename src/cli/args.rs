//! Definição da linha de comando (clap): o `Cli` e todos os enums de
//! subcomando. É o CONTRATO da CLI — o que o usuário pode pedir.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "schematize", version, about = "Ecosystem manager for Claude — skills, overdev, and more (Linux-first).")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
    /// Manage skills (a FEATURE of the app): install | update | remove | list.
    Skills {
        #[command(subcommand)]
        sub: SkillsCmd,
    },
    // --- Aliases de compat (ocultos): os antigos top-level de SKILLS agora vivem
    // sob `schematize skills <sub>`. Mantidos válidos pra não quebrar scripts/hooks/docs.
    /// (alias oculto de `skills install`)
    #[command(hide = true)]
    Install {
        names: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        with_recommended: bool,
    },
    /// (alias oculto de `skills update`)
    #[command(hide = true)]
    Update {
        names: Vec<String>,
        #[arg(long)]
        all: bool,
    },
    /// (alias oculto de `skills list`)
    #[command(hide = true)]
    List,
    /// (alias oculto de `skills remove`)
    #[command(hide = true)]
    Remove { name: String },
    /// Gera o ícone do app EM CÓDIGO (resiliente — sem rasterizar SVG). `--emit <png>` um tamanho;
    /// `--hicolor <dir>` a árvore freedesktop inteira (16..512). Usado pelo install.sh.
    #[command(hide = true)]
    Icon {
        /// Escreve um PNG único neste caminho.
        #[arg(long)]
        emit: Option<String>,
        /// Tamanho (px) do `--emit`.
        #[arg(long, default_value = "256")]
        size: u32,
        /// Gera a árvore hicolor completa (`<dir>/<N>x<N>/apps/schematize.png`) neste dir base.
        #[arg(long)]
        hicolor: Option<String>,
    },
    /// Overview dashboard: versions, agent, overdev, language, links.
    Status,
    /// Envia (OPT-IN) o relatório de diagnóstico REDIGIDO pro servidor (POST /diagnostics). Nada é
    /// enviado por padrão — só com este comando e após confirmar.
    Diagnostics {
        /// Pula a confirmação (envia direto).
        #[arg(long)]
        yes: bool,
    },
    /// Orçamento de concorrência: quantos agents/subagents do Claude a máquina aguenta SEM travar
    /// (CPU/RAM/load, contando OUTRAS instâncias do claude na máquina). Persiste ~/.schematize/agents.json.
    Agents {
        /// Sai em JSON (pra scripts/hooks) em vez da tabela legível.
        #[arg(long)]
        json: bool,
        /// Simula um SPLIT em K claudes principais e mostra quantos subagents cada um pode abrir.
        #[arg(long, value_name = "K")]
        split: Option<usize>,
    },
    /// Contas de git/GitHub, repositórios e o que ainda não saiu da máquina.
    Git {
        #[command(subcommand)]
        sub: GitCmd,
    },
    /// Inventário e limpeza do lixo recriável: artefato de build, cache de toolchain
    /// e camada de Docker — agrupado por DISCO (é o principal que costuma encher).
    Disco {
        #[command(subcommand)]
        sub: DiscoCmd,
    },
    /// Diagnose the environment (add --fix to repair what's safe).
    Doctor {
        #[arg(long)]
        fix: bool,
    },
    /// Debug the updater/versioning; or --collect a shareable, secret-safe debug report.
    Debug {
        /// Collect a FULL debug report (system, install, deps, config, skills, overdev, logs).
        #[arg(long)]
        collect: bool,
        /// Where to write the report (default: ~/.schematize/debug-report-<epoch>.txt).
        #[arg(long)]
        out: Option<String>,
        /// Print the whole report to stdout instead of writing a file (only with --collect).
        #[arg(long)]
        stdout: bool,
        /// Include NETWORK diagnostics (updater/rate-limit, catalog reach, doctor's github check).
        /// Off by default so the report is FAST even on a slow/blocked network.
        #[arg(long)]
        online: bool,
    },
    /// Archive de evolução: materializa a estrutura do <projeto>_archive/, extrai o chat da sessão
    /// pro chats/ e gera o context_agent/#N.txt (contexto portável). Roda no dir do projeto.
    Archive,
    /// Update schematize itself — atualiza o próprio schematize (o APP, o binário), não as skills.
    Upgrade {
        #[arg(long)]
        force: bool,
    },
    /// Show the latest posts from blog.schematize.net.
    News,
    /// Show aggregated notifications (app update, blog posts, outdated skills), grouped by scope.
    Notifications {
        /// Fetch from the network first (default: read the local cache only).
        #[arg(long)]
        sync: bool,
        /// Also show the ones already resolved (history).
        #[arg(long)]
        historico: bool,
        /// Mark every unread one as seen. Deletes nothing.
        #[arg(long)]
        lidas: bool,
        /// Mark one as resolved by id. It moves to history, it is not deleted.
        #[arg(long)]
        concluir: Option<String>,
    },
    /// Open the blog (blog.schematize.net) in the browser.
    Blog,
    /// Open a resource in the browser: site | blog | github.
    Open { target: String },
    /// Get/set the interface language (no args = show current; --list = all).
    Lang {
        code: Option<String>,
        #[arg(long)]
        list: bool,
    },
    /// Overdev mode — continuous dev until the checklist is done.
    Overdev {
        #[command(subcommand)]
        sub: Over,
    },
    /// Show recent git commits, flagging which are pushed (● pushed / ○ local).
    GitLog {
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Open the auxiliary HTML panel (overdev + index graph) in the browser.
    Panel,
    /// Graph tools (export the index graph, Obsidian-style).
    Graph {
        #[command(subcommand)]
        sub: GraphCmd,
    },
    /// Database builder backend: introspect a DB, emit SQL/migration, or print the schema graph.
    Db {
        #[command(subcommand)]
        sub: DbCmd,
    },
    /// Check for updates once (with --notify, fire a desktop notification).
    Check {
        #[arg(long)]
        notify: bool,
    },
    /// Resident agent: periodically checks and notifies (used by autostart).
    Agent,
    /// Bind the agent to the system (starts at login).
    Autostart {
        #[command(subcommand)]
        sub: Auto,
    },
    /// Dev environments: language runtimes (docker|mise|distro|official) + dev tools (claude|code|codex).
    Env {
        #[command(subcommand)]
        sub: EnvCmd,
    },
    /// Open the graphical window (same software as the CLI — just the GUI face).
    Gui,
    /// SSH keys: generate, list, export and manage keys in ~/.ssh (never leaks the private key).
    Ssh {
        #[command(subcommand)]
        sub: SshCmd,
    },
    /// Projects: list detected/pinned projects, pin/unpin, or drop a `.schematize` marker.
    Projects {
        #[command(subcommand)]
        sub: ProjectsCmd,
    },
    /// Log in to the schematize platform via the browser (OAuth device flow).
    Login,
    /// Log out (delete the local session).
    Logout,
    /// Show who is logged in (the account subject).
    Whoami,
}

/// Gestão de SKILLS — uma funcionalidade do app, agrupada sob `schematize skills`.
#[derive(Subcommand)]
pub(crate) enum SkillsCmd {
    /// Which skill VERSION shaped THIS project, and which fell behind the installed one.
    Applied {
        /// Record that <slug> was just applied here (the agent calls this when it finishes).
        #[arg(long)]
        mark: Option<String>,
    },
    /// Re-run a skill over this project to refresh its precepts (opens an agent).
    Rerun {
        /// Skill slug; omit to re-run every outdated one.
        slug: Option<String>,
    },
    /// Install one or more skills (or all with --all) from the latest release.
    Install {
        names: Vec<String>,
        #[arg(long)]
        all: bool,
        /// Also install any recommended (complementary) skills, e.g. engineering.
        #[arg(long)]
        with_recommended: bool,
    },
    /// Update installed skills to latest (all if no name/--all).
    Update {
        names: Vec<String>,
        #[arg(long)]
        all: bool,
    },
    /// List skills: installed vs latest available.
    List,
    /// Remove an installed skill.
    Remove { name: String },
    /// Create a new skill scaffold in ~/.claude/skills/schematize-<slug>/.
    New {
        /// slug (allow-list [a-z0-9-]) — vira a pasta schematize-<slug>.
        slug: String,
        /// Human name (title/descriptions); defaults to the slug.
        #[arg(long)]
        name: Option<String>,
        /// Dense description for the SKILL.md frontmatter.
        #[arg(long)]
        desc: Option<String>,
        /// Overwrite if the skill already exists.
        #[arg(long)]
        force: bool,
    },
    /// Edit an installed skill: --list its files, or --file <rel> to print one (or set it).
    Edit {
        slug: String,
        /// List the editable files (relative paths). Default action if nothing else is given.
        #[arg(long)]
        list: bool,
        /// A file relative to the skill root: prints it (or writes it with --set-from).
        #[arg(long)]
        file: Option<String>,
        /// Write the file from this source path (needs --file <rel>).
        #[arg(long)]
        set_from: Option<String>,
    },
    /// Fork an OFFICIAL skill (stash its base) so it can be edited without losing the original.
    Fork { slug: String },
    /// Compare a forked skill against the latest official (files changed + unified diff).
    Compare { slug: String },
}

#[derive(Subcommand)]
pub(crate) enum SshCmd {
    /// Generate a key pair (ed25519 by default; --rsa = rsa 4096) into ~/.ssh/<name>.
    Gen {
        name: String,
        /// Use RSA 4096 instead of the recommended ed25519.
        #[arg(long)]
        rsa: bool,
        /// Comment embedded in the key (default: schematize:<user>@<host>).
        #[arg(long)]
        comment: Option<String>,
        /// Also add the public key to your GitHub account (gh must be authenticated).
        #[arg(long)]
        github: bool,
        /// Also load the key into the ssh-agent (ssh-add).
        #[arg(long)]
        agent: bool,
        /// Overwrite an existing key with the same name.
        #[arg(long)]
        force: bool,
    },
    /// List keys in ~/.ssh (name, type, fingerprint, comment). Never reads the private key.
    List,
    /// Print the PUBLIC key (paste it on GitHub/servers); --copy sends it to the clipboard.
    /// With --bitwarden, export the key to Bitwarden instead (item in the vault if `bw` is
    /// unlocked, else a mode-600 import JSON) — the PRIVATE key never hits stdout.
    Export {
        name: String,
        #[arg(long)]
        copy: bool,
        /// Export to Bitwarden (vault item via `bw`, or a mode-600 import JSON as fallback).
        #[arg(long)]
        bitwarden: bool,
        /// Import-JSON output path (only with --bitwarden fallback). Default ~/.schematize/bw-import-<name>.json.
        #[arg(long)]
        out: Option<String>,
    },
    /// Deploy WITHOUT pasting the key: `ssh -i <managed key> user@host [-- <remote cmd...>]`.
    /// Inherits the terminal, never prints the private key. No command = interactive session.
    /// Ex.: schematize ssh run deploy root@host -- 'cd /srv/app && git pull && ./deploy.sh'
    Run {
        name: String,
        target: String,
        /// Remote command to run (everything after `--`). Empty = interactive shell.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Install the PUBLIC key into the remote host's ~/.ssh/authorized_keys (bootstrap access).
    /// Requires you already have access to the host (another key/agent/password).
    Authorize { name: String, target: String },
    /// Remove a key pair (private + public) with confirmation.
    Rm { name: String },
    /// Add an existing PUBLIC key to your GitHub account (gh must be authenticated).
    Github { name: String },
}

/// Gestão de PROJETOS — listar, fixar/desafixar (pin) e marcar pastas como projeto.
#[derive(Subcommand)]
pub(crate) enum ProjectsCmd {
    /// List every project (pinned + detected from the dev dirs), flagging the pinned.
    List,
    /// Pin a folder as a first-class project (always listed, even without a git marker).
    Add { path: String },
    /// Unpin a previously pinned folder.
    Remove { path: String },
    /// Drop a `.schematize` marker file in the folder (default: cwd) → a project that
    /// stops the scan from descending (turns an umbrella into ONE project).
    Mark { path: Option<String> },
    /// Remove the `.schematize` marker from the folder (default: cwd).
    Unmark { path: Option<String> },
}

/// `schematize disco` — inventário e limpeza do lixo recriável (build, cache, docker).
/// `schematize git` — contas, repositórios e o que ainda não saiu da máquina.
#[derive(Subcommand)]
pub(crate) enum GitCmd {
    /// Lista as contas cadastradas.
    Accounts,
    /// Cadastra (ou substitui) uma conta.
    Add {
        /// Rótulo curto e sem espaço ("pessoal", "volucer").
        rotulo: String,
        #[arg(long)]
        usuario: String,
        #[arg(long)]
        email: String,
        /// Arquivo da chave em ~/.ssh (sem isto, a conta usa o `gh`).
        #[arg(long)]
        chave: Option<String>,
        /// Host do serviço (default github.com).
        #[arg(long)]
        servico: Option<String>,
    },
    /// DETECTA contas já presentes na máquina (`gh`, git config, ~/.ssh, e-mail dos repos)
    /// e mostra o que daria pra cadastrar. Só sugere; `--add` é que grava.
    Detect {
        /// Cadastra as sugestões que ainda não existem.
        #[arg(long)]
        add: bool,
    },
    /// Remove uma conta pelo rótulo.
    Remove { rotulo: String },
    /// Aplica uma conta ao repositório do diretório atual.
    Use {
        rotulo: String,
        /// Nome do remoto (default origin).
        #[arg(long)]
        remoto: Option<String>,
    },
    /// Escreve o alias SSH da conta no ~/.ssh/config.
    SshConfig { rotulo: String },
    /// Lista os repositórios do serviço (via `gh`).
    Repos {
        /// Só desta conta (default: todas).
        rotulo: Option<String>,
        #[arg(long, default_value_t = 50)]
        limite: usize,
    },
    /// O que ainda NÃO saiu da máquina, projeto a projeto.
    Status,
    /// Commits do projeto atual, marcando os já enviados.
    Log {
        #[arg(long, default_value_t = 20)]
        limite: usize,
    },
}

#[derive(Subcommand)]
pub(crate) enum DiscoCmd {
    /// Lista o que dá pra recuperar, agrupado por DISCO e por tipo.
    List {
        /// Só o que está parado há pelo menos N dias.
        #[arg(long, default_value_t = 0)]
        min_dias: u64,
    },
    /// Apaga os artefatos que casam com os filtros (mostra a lista antes).
    Clean {
        /// Só o que está parado há pelo menos N dias.
        #[arg(long, default_value_t = 30)]
        min_dias: u64,
        /// Filtra por tipo (ex.: "target", "node_modules", "cache").
        #[arg(long)]
        tipo: Option<String>,
        /// Só neste disco (ponto de montagem, ex.: "/" ou "/home").
        #[arg(long)]
        montagem: Option<String>,
        /// Não perguntar.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Uso e podas do Docker.
    Docker {
        /// Executa uma poda pelo rótulo (sem isto, só lista).
        #[arg(long)]
        podar: Option<String>,
        /// Não perguntar (não vale pras podas que apagam dados).
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum EnvCmd {
    /// List languages and dev tools, install paths available here, and install status.
    List,
    /// Install a language runtime (via a method) OR a dev tool (claude|code|codex; --method ignored).
    Install {
        /// language slug (go|rust|...) or tool slug (claude|code|codex).
        lang: String,
        /// docker | mise | distro | official — required for languages; ignored for tools.
        #[arg(long)]
        method: Option<String>,
        /// Print everything and execute nothing.
        #[arg(long)]
        dry_run: bool,
        /// Skip the interactive confirmation (run without asking).
        #[arg(long)]
        yes: bool,
    },
    /// Remove a language environment (auto-detects the method) OR a dev tool (--method ignored).
    Remove {
        /// language slug (go|rust|...) or tool slug (claude|code|codex).
        lang: String,
        #[arg(long)]
        method: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum GraphCmd {
    /// Export the index as an Obsidian vault (markdown + [[wikilinks]]).
    Obsidian {
        /// Output directory (default: <project>_archive/obsidian).
        #[arg(long)]
        out: Option<String>,
    },
}

/// Backend do "database builder" — introspecta, gera SQL/migration e o grafo do schema.
#[derive(Subcommand)]
pub(crate) enum DbCmd {
    /// Introspect a database (SQLite file or Postgres conn) and print a summary.
    Introspect {
        /// Path to a SQLite file to introspect.
        #[arg(long)]
        sqlite: Option<String>,
        /// Postgres connection string (uses `psql` from PATH).
        #[arg(long)]
        postgres: Option<String>,
        /// Also print the schema as pretty JSON (for the GUI / piping).
        #[arg(long)]
        json: bool,
    },
    /// Emit SQL (CREATE/ALTER/INDEX) — or a migration with --migration — from a schema source.
    Sql {
        /// Read the schema from a JSON file (as saved by the GUI).
        #[arg(long)]
        from: Option<String>,
        /// Or introspect this SQLite file as the source.
        #[arg(long)]
        sqlite: Option<String>,
        /// Or introspect this Postgres conn as the source.
        #[arg(long)]
        postgres: Option<String>,
        /// Emit an expand-contract migration (up/down) instead of plain SQL.
        #[arg(long)]
        migration: bool,
    },
    /// Print the schema graph (nodes/edges: table = node, FK = edge) from a schema source.
    Graph {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        sqlite: Option<String>,
        #[arg(long)]
        postgres: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum Auto {
    /// Enable and start the agent (systemd --user + XDG autostart).
    Enable,
    /// Disable and remove the autostart.
    Disable,
}

#[derive(Subcommand)]
pub(crate) enum Over {
    /// Register the hooks (Stop + AskUserQuestion veto) in settings.json.
    Enable,
    /// Remove the overdev hooks from settings.json.
    Disable,
    /// Start a run in the current directory: `schematize overdev start "<goal>"`.
    Start {
        objetivo: Vec<String>,
        #[arg(long)]
        max: Option<u64>,
    },
    /// Abre um TERMINAL interativo já neste projeto, com o `claude` pronto e o bypass de
    /// permissões ligado. Quando o claude sai, o shell continua aberto na pasta certa.
    Terminal,
    /// SUPERVISIONA o run deste diretório: se o agente morrer (contexto/crash/janela fechada)
    /// com item de máquina aberto, RELANÇA. É a rede que o Stop hook não cobre — ele só age
    /// quando o agente TENTA encerrar o turno, não quando o processo simplesmente acaba.
    Supervise {
        /// Teto de relançamentos (guardrail anti-loop).
        #[arg(long)]
        max: Option<u32>,
    },
    /// SPLIT do checklist em K arquivos `checklist/part-N.md` (pastas multi-arquivo) pra rodar
    /// multiagents. Respeita o governador (`schematize agents`): mostra quantos subagents por claude
    /// e recusa passar do teto seguro. `--dispatch` lança os K claudes (cada um no seu part).
    Split {
        /// Em quantos claudes principais dividir (2, 4, …).
        k: usize,
        /// Lança os K claudes em terminais externos, cada um no seu part-N.md.
        #[arg(long)]
        dispatch: bool,
        /// Ignora o teto do governador (perigoso — pode travar a máquina).
        #[arg(long)]
        force: bool,
    },
    /// (Stop hook) reject stopping while there is an open item.
    Check,
    /// (PreToolUse hook) veto AskUserQuestion during overdev.
    Guard,
    /// Show the run state.
    Status,
    /// Mark the first open item matching the text as on-hold.
    Hold { texto: Vec<String> },
    /// Park a question (log it in the base txt) and mark the item on-hold.
    Park { item: String, pergunta: Vec<String> },
    /// Human closes a `- [H ]` item → `- [H x]`: by text and/or `--done <n>`.
    Human {
        /// Text that the human item must contain (optional if --done is given).
        texto: Vec<String>,
        /// Close the Nth open human item (1-based) instead of matching text.
        #[arg(long)]
        done: Option<usize>,
    },
    /// Attach a human note (correction prompt / per-task point) in .schematize/overdev/NOTAS.md.
    Note {
        texto: Vec<String>,
        /// Note kind: correcao (correction prompt) | task (per-task point). Default: correcao.
        #[arg(long, default_value = "correcao")]
        kind: String,
    },
    /// Answer a human item with text — this RELEASES the machine item it was blocking.
    Answer {
        /// Nth open human item (1-based), or a text fragment of it.
        alvo: String,
        /// The answer / decision. Goes to the checklist and to DECISOES.md.
        texto: Vec<String>,
    },
    /// Refuse a human item — the machine item it blocked is CANCELLED, not resumed.
    Refuse {
        /// Nth open human item (1-based), or a text fragment of it.
        alvo: String,
        /// Why it is not applicable.
        texto: Vec<String>,
    },
    /// Add a demand to the inbox WITHOUT touching the checklist (safe while an agent runs).
    Add {
        texto: Vec<String>,
    },
    /// Inbox: list, organize a demand into items, or merge them into the checklist.
    Caixa {
        #[command(subcommand)]
        sub: CaixaCmd,
    },
    /// End the run (hooks become inert again).
    Stop,
    /// Run overdev with an attached `claude` agent in a PTY (monitors + auto-continue).
    Run {
        /// Max number of `continue` nudges injected when the agent goes idle.
        #[arg(long)]
        max: Option<u64>,
        /// Skip the confirmation prompt (the agent WILL touch this project).
        #[arg(long)]
        yes: bool,
    },
    /// Snapshot the `.schematize/overdev/` artifacts into the local DB (versioned backup).
    Snapshot,
    /// List the local DB snapshot history for this project (newest first).
    History {
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Restore a snapshot (by id) back to its original path.
    Restore { id: i64 },
    /// Inject `/eng-load` into a `claude` session to load the engineering precepts.
    Load,
    /// Inject `/eng-index` into a `claude` session to (re)index the project.
    Index,
    /// Print the completion log (HH:MM:SS local + item) from .schematize/overdev/completions.json.
    Log,
}

/// Subcomandos da CAIXA DE ENTRADA do overdev.
///
/// Três estágios separados de propósito: capturar é instantâneo e infalível,
/// organizar é lento (um agente pensa) e fundir é curto e serializado. Misturá-los
/// num comando só significaria segurar a trava do checklist enquanto um agente
/// pensa — ou seja, travar o projeto. Ver `overdev::caixa`.
#[derive(Subcommand)]
pub(crate) enum CaixaCmd {
    /// Show what is captured and not yet in the checklist.
    List,
    /// Record the items an agent extracted from a demand (moves it to `processado`).
    Organizar {
        /// Demand id (see `caixa list`).
        id: String,
        /// One checklist item; repeat the flag for each.
        #[arg(long = "item", required = true)]
        itens: Vec<String>,
    },
    /// Merge every organized demand into the checklist (atomic, under lock).
    Merge,
    /// Open an agent in a terminal to organize the pending demands.
    Agente,
}
