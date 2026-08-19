//! schematize — gerenciador do ecossistema para o Claude (skills, overdev e mais).
//! O quê: CLI multi-idioma que instala/versiona skills, roda o overdev, diagnostica
//! o ambiente (doctor), atualiza a si mesmo (upgrade), mostra status e o blog.
//! Onde: ponto de entrada; despacha pros módulos da lib `schematize`.

use clap::{Parser, Subcommand};
use schematize::agentrun::AgentRunner;
use schematize::i18n::{t, tf};
use schematize::{
    account, agent, agentrun, autostart, config, database, debug, debugreport, doctor, environments,
    githist, i18n, links, market, news, notifications, overdev, overdevdb, panel, projects, registry,
    skilledit, skills, sshkeys, status, upgrade, util,
};
use std::io::{self, BufRead, Write};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "schematize", version, about = "Ecosystem manager for Claude — skills, overdev, and more (Linux-first).")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
    /// Overview dashboard: versions, agent, overdev, language, links.
    Status,
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
    Notifications,
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
enum SkillsCmd {
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
enum SshCmd {
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
enum ProjectsCmd {
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

#[derive(Subcommand)]
enum EnvCmd {
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
enum GraphCmd {
    /// Export the index as an Obsidian vault (markdown + [[wikilinks]]).
    Obsidian {
        /// Output directory (default: <project>_archive/obsidian).
        #[arg(long)]
        out: Option<String>,
    },
}

/// Backend do "database builder" — introspecta, gera SQL/migration e o grafo do schema.
#[derive(Subcommand)]
enum DbCmd {
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
enum Auto {
    /// Enable and start the agent (systemd --user + XDG autostart).
    Enable,
    /// Disable and remove the autostart.
    Disable,
}

#[derive(Subcommand)]
enum Over {
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

fn resolve(cat: &[registry::Item], names: &[String], all: bool) -> Vec<registry::Item> {
    if all || names.is_empty() {
        cat.to_vec()
    } else {
        names.iter().filter_map(|n| registry::find(cat, n)).collect()
    }
}

/// `schematize lang [code] [--list]`.
/// `schematize agents` — imprime o orçamento de concorrência e persiste ~/.schematize/agents.json.
fn agents_cmd(json: bool, split: Option<usize>) -> Result<(), String> {
    let b = schematize::agents::budget();
    let _ = schematize::agents::persist(&b); // best-effort: outros (Claude/overdev/GUI) leem daqui.

    if json {
        let plan = split.map(|k| b.split_plan(k));
        let mut v = serde_json::json!({
            "total_cap": b.total_cap, "available": b.available,
            "cpu_cap": b.cpu_cap, "ram_cap": b.ram_cap, "load_cap": b.load_cap,
            "threads": b.snap.threads, "mem_available_mb": b.snap.mem_available_mb,
            "load1": b.snap.load1, "running_claudes": b.snap.running_claudes,
            "ram_tight": b.ram_tight,
        });
        if let Some(p) = plan {
            v["split"] = serde_json::json!({
                "mains": p.mains, "subagents_each": p.subagents_each, "total_used": p.total_used
            });
        }
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(());
    }

    let gb = |mb: u64| format!("{:.1} GB", mb as f64 / 1024.0);
    println!("\x1b[1mOrçamento de concorrência do Claude (máquina inteira)\x1b[0m");
    println!("  threads lógicos      : {}", b.snap.threads);
    println!("  reserva (respiro)    : {}", b.params.reserve);
    println!("  RAM disponível       : {}  (≈{} por agent, −{:.0}% de margem)", gb(b.snap.mem_available_mb), gb(b.params.mb_per_agent), b.params.ram_margin * 100.0);
    println!("  load atual (1min)    : {:.2}", b.snap.load1);
    println!("  claudes rodando AGORA: {}  (esta janela + outras + subagents)", b.snap.running_claudes);
    println!("  ─────────────────────");
    println!("  teto por CPU         : {}", b.cpu_cap);
    println!("  teto por RAM         : {}{}", b.ram_cap, if b.ram_tight { "  \x1b[33m(RAM apertada — cuidado com swap)\x1b[0m" } else { "" });
    println!("  teto por load        : {}", b.load_cap);
    println!("  \x1b[1mTETO TOTAL seguro    : {}\x1b[0m  (o menor dos três)", b.total_cap);
    println!("  \x1b[1;32mDISPONÍVEL p/ lançar : {}\x1b[0m  (teto − já rodando)", b.available);
    if let Some(k) = split {
        let p = b.split_plan(k);
        println!("\n  \x1b[1mSplit em {} claude(s) principal(is):\x1b[0m {} subagents cada  (total {}/{} do teto)", p.mains, p.subagents_each, p.total_used, p.cap);
    }
    Ok(())
}

fn lang_cmd(code: Option<String>, list: bool) -> Result<(), String> {
    if list {
        println!("{}", t("lang.available"));
        for (c, name, _) in i18n::LANGS {
            println!("  {c:<4} {name}");
        }
        return Ok(());
    }
    match code {
        Some(c) => {
            if !i18n::is_supported(&c) {
                return Err(tf("lang.unknown", &[("code", &c)]));
            }
            i18n::set_lang(&c)?;
            let name = i18n::name_of(&c).unwrap_or("");
            println!("{}", tf("lang.set", &[("code", &c), ("langname", name)]));
            println!("{}", t("lang.restart_gui"));
            Ok(())
        }
        None => {
            let c = i18n::current_code();
            let name = i18n::name_of(&c).unwrap_or("");
            println!("{}", tf("lang.current", &[("code", &c), ("langname", name)]));
            Ok(())
        }
    }
}

/// `schematize overdev run [--max N] [--yes]` — dispara o `claude` acoplado no
/// diretório atual e monitora (auto-continue). Guardrail: mostra o comando do
/// agente e confirma antes (o agente MEXE no projeto), a menos de `--yes`.
/// `schematize overdev split K` — divide o checklist em K parts e (com --dispatch) lança K claudes,
/// tudo dentro do teto seguro do governador (`schematize agents`).
fn overdev_split(k: usize, dispatch: bool, force: bool) -> Result<(), String> {
    let project = std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))?;
    let b = schematize::agents::budget();
    let plan = b.split_plan(k);

    println!("Governador de concorrência (máquina inteira):");
    println!("  teto seguro: {} · rodando agora: {} · disponível: {}", b.total_cap, b.snap.running_claudes, b.available);
    println!("  split em {} claude(s): {} subagents cada (total {}/{} do teto)", plan.mains, plan.subagents_each, plan.total_used, plan.cap);

    if k > b.total_cap && !force {
        return Err(format!(
            "{k} claudes principais passa do teto seguro ({}). Reduza o K ou use --force (pode travar a máquina).",
            b.total_cap
        ));
    }
    if dispatch && b.available < k && !force {
        return Err(format!(
            "só há {} slot(s) livre(s) na máquina agora (teto {} − {} rodando); lançar {k} travaria. Espere liberar ou use --force.",
            b.available, b.total_cap, b.snap.running_claudes
        ));
    }

    let res = overdev::split(&project, k)?;
    println!("\n✓ dividido: {} itens em {} parte(s):", res.moved, res.parts.len());
    for (i, (f, n)) in res.parts.iter().zip(&res.per_part).enumerate() {
        println!("  part {:>2}: {n:>3} item(ns)  → {}", i + 1, f.display());
    }

    if !dispatch {
        println!("\nRevise os parts e rode com --dispatch pra lançar os {k} claudes (ou abra cada um você mesmo).");
        return Ok(());
    }

    println!("\nLançando {k} claude(s) — um por part…");
    for (i, f) in res.parts.iter().enumerate() {
        let rel = f.strip_prefix(&project).unwrap_or(f);
        let prompt = format!(
            "Rode o overdev deste projeto cuidando APENAS do arquivo `{}` (sua fatia do split). Feche \
             TODOS os itens `- [ ]` dele com prova, seguindo a disciplina do overdev. Você pode usar até \
             {} subagents em paralelo — NÃO ultrapasse, pra não travar a máquina (há outros claudes \
             rodando as outras fatias). Não toque nos outros part-*.md.",
            rel.display(),
            plan.subagents_each
        );
        match agentrun::launch_prompt_in_terminal(&project, &prompt) {
            Ok(_) => println!("  ✓ claude {} lançado ({})", i + 1, rel.display()),
            Err(e) => println!("  ✗ claude {} falhou: {e}", i + 1),
        }
    }
    Ok(())
}

fn overdev_run(max: Option<u64>, yes: bool) -> Result<(), String> {
    let project = std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))?;
    let max = max.unwrap_or(agentrun::DEFAULT_MAX_NUDGES);
    let objetivo = overdev::objetivo_at(&project).unwrap_or_default();
    let runner = agentrun::ClaudeRunner;
    println!("Vai disparar o agente acoplado neste projeto:");
    println!("  projeto: {}", project.display());
    println!("  comando: {}", runner.command_line(&objetivo));
    println!("  auto-continue: até {max} nudge(s) quando o agente ficar ocioso com item aberto.");
    if !yes && !confirm("Disparar o agente `claude` acoplado neste projeto? [s/N]") {
        return Err("cancelado.".to_string());
    }
    agentrun::run_attached(&project, &runner, max)
}

/// cwd como raiz do projeto (erro claro se inacessível).
fn cwd_project() -> Result<std::path::PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))
}

/// `schematize overdev snapshot` — grava no DB local as versões novas dos artefatos.
fn overdev_snapshot() -> Result<(), String> {
    let project = cwd_project()?;
    let n = overdevdb::snapshot(&project)?;
    if n == 0 {
        println!("nenhuma mudança — nada novo pra versionar.");
    } else {
        println!("{n} snapshot(s) novo(s) gravado(s) no DB local.");
    }
    Ok(())
}

/// `schematize overdev history [--limit N]` — tabela do histórico do projeto.
fn overdev_history(limit: usize) -> Result<(), String> {
    let project = cwd_project()?;
    let hist = overdevdb::history(&project, limit)?;
    if hist.is_empty() {
        println!("sem snapshots pra este projeto ainda (rode `schematize overdev snapshot`).");
        return Ok(());
    }
    println!("{:>6}  {:<19}  {:>8}  {}", "id", "quando", "bytes", "arquivo");
    for m in hist {
        println!("{:>6}  {:<19}  {:>8}  {}", m.id, fmt_ts(m.ts), m.size, m.file);
    }
    println!("(veja um: `schematize overdev restore <id>`)");
    Ok(())
}

/// `schematize overdev restore <id>` — regrava o snapshot no caminho original.
fn overdev_restore(id: i64) -> Result<(), String> {
    let project = cwd_project()?;
    let dest = overdevdb::restore(id, &project)?;
    println!("snapshot {id} restaurado em {}", dest.display());
    Ok(())
}

/// Formata um epoch secs em `AAAA-MM-DD HH:MM` (UTC, sem crate de data).
fn fmt_ts(ts: i64) -> String {
    // Cálculo civil a partir do epoch (algoritmo de Howard Hinnant), UTC.
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let (h, mi) = (secs / 3600, (secs % 3600) / 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

/// Offset local em segundos (via `date +%z`, ex.: `-0300`). Fallback 0 (UTC) se
/// o `date` falhar ou vier num formato inesperado.
fn local_offset_secs() -> i64 {
    let out = match std::process::Command::new("date").arg("+%z").output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return 0,
    };
    let s = String::from_utf8_lossy(&out);
    let s = s.trim();
    // Formato esperado: sinal + HHMM (ex.: "-0300", "+0530").
    if s.len() < 5 {
        return 0;
    }
    let sign: i64 = if s.starts_with('-') { -1 } else { 1 };
    let digits: Vec<char> = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return 0;
    }
    let hh: i64 = format!("{}{}", digits[0], digits[1]).parse().unwrap_or(0);
    let mm: i64 = format!("{}{}", digits[2], digits[3]).parse().unwrap_or(0);
    sign * (hh * 3600 + mm * 60)
}

/// Hora local `HH:MM:SS` de um epoch (secs), dado o offset local. PURO/testável.
fn fmt_hms(ts: i64, offset: i64) -> String {
    let secs = (ts + offset).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// `schematize overdev log` — lista as conclusões (`- [x]`) do CHECKLIST com a hora
/// local em que foram detectadas (lê `.overdev/completions.json`, sem gravar).
fn overdev_log() {
    let project = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let comps = overdev::completions(&project);
    if comps.is_empty() {
        println!("sem conclusões registradas ainda (.schematize/overdev/completions.json).");
        return;
    }
    let off = local_offset_secs();
    println!("conclusões ({}):", comps.len());
    for c in &comps {
        println!("  {}  {}", fmt_hms(c.ts, off), c.text);
    }
}

/// `schematize overdev load|index` — dispara uma sessão `claude` one-shot no cwd
/// com o comando dado. Se `claude` não estiver no PATH, só imprime a dica.
fn overdev_agent_cmd(cmd: &str) -> Result<(), String> {
    if !agentrun::claude_in_path() {
        println!("`claude` não está no PATH. Rode manualmente na pasta do projeto: claude {cmd}");
        return Ok(());
    }
    println!("disparando sessão `claude` no diretório atual com: {cmd}");
    match std::process::Command::new("claude").arg(cmd).status() {
        Ok(st) if st.success() => Ok(()),
        Ok(st) => {
            println!("a sessão `claude` saiu com {st}. Se preciso, rode à mão: claude {cmd}");
            Ok(())
        }
        Err(e) => {
            println!("não consegui disparar `claude` ({e}). Rode à mão: claude {cmd}");
            Ok(())
        }
    }
}

/// `schematize debug [--collect] [--out <path>] [--stdout]`.
/// Sem `--collect`: o debug do updater (comportamento atual). Com `--collect`: monta o
/// relatório completo (secret-safe) e grava um arquivo modo 600 (ou imprime com `--stdout`).
fn debug_cmd(collect: bool, out: Option<String>, stdout: bool, online: bool) -> Result<(), String> {
    if !collect {
        debug::run();
        return Ok(());
    }
    if stdout {
        print!("{}", debugreport::collect(online));
        return Ok(());
    }
    let path = debugreport::write_report(out.as_deref().map(std::path::Path::new), online)?;
    println!("Relatório de debug gravado em: {}", path.display());
    println!("  {}", debugreport::short_summary());
    println!("  (modo 600 — segredos redigidos automaticamente; revise antes de compartilhar.)");
    Ok(())
}

/// `schematize git-log [--limit N]` — commits recentes marcando push (●/○).
fn git_log(limit: usize) {
    let root = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cwd inacessível: {e}");
            return;
        }
    };
    let cs = githist::commits(&root, limit);
    if cs.is_empty() {
        println!("sem commits (ou não é um repositório git).");
        return;
    }
    for c in &cs {
        let mark = if c.pushed { '●' } else { '○' };
        println!("{mark} {}  {:<10}  {}  {}", c.short, c.date, c.author, c.subject);
    }
    match githist::upstream(&root) {
        Some(u) => println!(
            "\nbranch {} → {} (ahead {}, behind {})  [● pushado · ○ local]",
            u.branch,
            u.remote.as_deref().unwrap_or("?"),
            u.ahead,
            u.behind
        ),
        None => println!("\nbranch sem upstream (nenhum commit pushado)  [○ local]"),
    }
}

/// Confirmação interativa (y/N). Falha fechada: erro/EOF/qualquer coisa ≠ sim = não.
fn confirm(prompt: &str) -> bool {
    print!("{prompt} ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes" | "s" | "sim")
}

/// `schematize ssh <sub>` — gestão de chaves SSH. Nunca imprime a chave privada.
fn ssh_cmd(sub: SshCmd) -> Result<(), String> {
    match sub {
        SshCmd::Gen { name, rsa, comment, github, agent, force } => {
            let kind = if rsa { sshkeys::KeyKind::Rsa4096 } else { sshkeys::KeyKind::Ed25519 };
            let info = sshkeys::generate(&name, kind, comment.as_deref(), None, force)?;
            println!("{}", tf("ssh.generated", &[("name", &info.name), ("kind", &info.kind)]));
            println!("{}", tf("ssh.fingerprint", &[("fp", &info.fingerprint)]));
            // Prova de entropia: nível de segurança + linha do ssh-keygen -l (bits + tipo).
            println!("entropia: {}", sshkeys::entropy_note(kind));
            if let Ok(proof) = sshkeys::proof_line(&name) {
                println!("prova (ssh-keygen -l): {proof}");
            }
            if agent {
                if sshkeys::add_to_agent(&name) {
                    println!("{}", t("ssh.agent_ok"));
                } else {
                    println!("{}", t("ssh.agent_fail"));
                }
            }
            if github {
                match sshkeys::add_to_github(&name) {
                    Ok(()) => println!("{}", tf("ssh.github_ok", &[("name", &name)])),
                    Err(e) => eprintln!("{}", tf("err.prefix", &[("e", &e)])),
                }
            }
            Ok(())
        }
        SshCmd::List => {
            let keys = sshkeys::list();
            if keys.is_empty() {
                println!("{}", t("ssh.list_empty"));
                return Ok(());
            }
            println!("{}", t("ssh.list_header"));
            for k in keys {
                println!("  {:<20} {:<8} {}  {}", k.name, k.kind, k.fingerprint, k.comment);
            }
            Ok(())
        }
        SshCmd::Export { name, copy, bitwarden, out } => {
            // --bitwarden: exporta pro cofre/arquivo (NUNCA imprime a privada).
            if bitwarden {
                let out_path = out.as_deref().map(std::path::Path::new);
                let msg = sshkeys::export_bitwarden(&name, out_path)?;
                println!("{msg}");
                return Ok(());
            }
            let pubkey = sshkeys::export_public(&name)?;
            println!("{pubkey}");
            if copy {
                if sshkeys::copy_to_clipboard(&pubkey) {
                    println!("{}", t("ssh.copied"));
                } else {
                    eprintln!("{}", t("ssh.copy_fail"));
                }
            }
            Ok(())
        }
        SshCmd::Run { name, target, command } => {
            // Deploy sem chave inline: usa a privada gerenciada só via `-i` (nunca a imprime).
            let code = sshkeys::run_ssh(&name, &target, &command)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        SshCmd::Authorize { name, target } => {
            sshkeys::authorize(&name, &target)?;
            println!("chave pública '{name}' instalada em {target}:~/.ssh/authorized_keys");
            println!("teste o acesso: schematize ssh run {name} {target} -- 'echo ok'");
            Ok(())
        }
        SshCmd::Rm { name } => {
            sshkeys::valid_name(&name)?;
            if !confirm(&tf("ssh.confirm_rm", &[("name", &name)])) {
                println!("{}", t("ssh.aborted"));
                return Ok(());
            }
            sshkeys::remove(&name)?;
            println!("{}", tf("ssh.removed", &[("name", &name)]));
            Ok(())
        }
        SshCmd::Github { name } => {
            sshkeys::add_to_github(&name)?;
            println!("{}", tf("ssh.github_ok", &[("name", &name)]));
            Ok(())
        }
    }
}

/// Canonicaliza um caminho (relativo → absoluto); fallback: o próprio literal.
fn canon_or(path: &str) -> String {
    std::fs::canonicalize(path)
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| path.to_string())
}

/// `schematize projects <sub>` — lista/fixa/marca projetos.
fn projects_cmd(sub: ProjectsCmd) -> Result<(), String> {
    match sub {
        ProjectsCmd::List => {
            let dev_dirs = config::dev_dirs();
            let pinned = config::projects();
            let projs = projects::scan_with_pins(&dev_dirs, &pinned);
            if projs.is_empty() {
                println!("Nenhum projeto encontrado (cadastre dev_dirs ou fixe com `projects add`).");
                return Ok(());
            }
            println!("Projetos ({}):", projs.len());
            for p in &projs {
                let flag = if p.marker == "pinned" { "[fixado] " } else { "" };
                println!("  {flag}{}  {}  ({})", p.name, p.path, p.marker);
            }
            Ok(())
        }
        ProjectsCmd::Add { path } => {
            let canon = canon_or(&path);
            config::pin_project(&path);
            println!("Fixado: {canon}");
            Ok(())
        }
        ProjectsCmd::Remove { path } => {
            config::unpin_project(&path);
            println!("Desafixado: {}", canon_or(&path));
            Ok(())
        }
        ProjectsCmd::Mark { path } => {
            let dir = path.unwrap_or_else(|| ".".to_string());
            let dir = canon_or(&dir);
            let marker = std::path::Path::new(&dir).join(".schematize");
            std::fs::write(&marker, "{}\n").map_err(|e| format!("falha ao criar marcador: {e}"))?;
            println!("Marcado como projeto: {}", marker.display());
            Ok(())
        }
        ProjectsCmd::Unmark { path } => {
            let dir = path.unwrap_or_else(|| ".".to_string());
            let dir = canon_or(&dir);
            let marker = std::path::Path::new(&dir).join(".schematize");
            if marker.exists() {
                std::fs::remove_file(&marker).map_err(|e| format!("falha ao remover marcador: {e}"))?;
                println!("Marcador removido: {}", marker.display());
            } else {
                println!("Sem marcador em {}", marker.display());
            }
            Ok(())
        }
    }
}

/// `schematize notifications` — imprime as notificações agregadas, agrupadas por escopo.
fn notifications_cmd() {
    let all = notifications::collect();
    if all.is_empty() {
        println!("Sem notificações no momento.");
        return;
    }
    let global: Vec<_> = all
        .iter()
        .filter(|n| matches!(n.scope, notifications::NotifScope::Global))
        .collect();
    let personal: Vec<_> = all
        .iter()
        .filter(|n| matches!(n.scope, notifications::NotifScope::Personal))
        .collect();

    let print_group = |titulo: &str, ns: &[&notifications::Notif]| {
        if ns.is_empty() {
            return;
        }
        println!("{titulo} ({}):", ns.len());
        for n in ns {
            println!("  • [{}] {}", n.kind, n.title);
            if !n.body.trim().is_empty() {
                println!("    {}", n.body);
            }
            if let Some(a) = &n.action {
                println!("    → {a}");
            }
        }
    };
    print_group("Globais", &global);
    print_group("Pessoais", &personal);
}

/// `schematize login` — autentica via OAuth device flow: inicia o fluxo, mostra o
/// `user_code` + a URL de verificação, e faz o polling respeitando `interval`/`slow_down`
/// até autorizar (Ok), negar (Denied) ou expirar. Salva a sessão em `~/.schematize/auth.json`.
fn login_cmd() -> Result<(), String> {
    if let Some(sub) = account::account_sub() {
        println!("Você já está logado como {sub}. (Para trocar de conta: `schematize logout`.)");
        return Ok(());
    }
    let dl = account::device_start()?;
    println!("Para entrar, abra no navegador:");
    println!("  {}", dl.verification_uri);
    println!("E informe o código: {}", dl.user_code);
    if dl.verification_uri_complete != dl.verification_uri {
        println!("\n(ou abra direto, já com o código: {})", dl.verification_uri_complete);
    }
    // Best-effort: já abre o navegador na URL completa (não bloqueia).
    util::open_url(&dl.verification_uri_complete);
    println!("\nAguardando você autorizar no navegador...");

    let mut interval = dl.interval.max(1);
    let deadline = util::now_unix() + dl.expires_in;
    loop {
        if util::now_unix() >= deadline {
            return Err("o código expirou — rode `schematize login` de novo.".to_string());
        }
        std::thread::sleep(Duration::from_secs(interval));
        match account::device_poll_once(&dl.device_code) {
            Ok(account::PollResult::Pending) => continue,
            Ok(account::PollResult::SlowDown) => {
                interval += 5; // servidor pediu pra desacelerar.
                continue;
            }
            Ok(account::PollResult::Denied) => {
                return Err("autorização negada. Nada foi salvo.".to_string());
            }
            Ok(account::PollResult::Expired) => {
                return Err("o código expirou — rode `schematize login` de novo.".to_string());
            }
            Ok(account::PollResult::Ok(tokens)) => {
                account::save_tokens(&tokens)?;
                println!("\n✓ Login efetuado! Você está logado como {}.", tokens.sub);
                return Ok(());
            }
            // Falha de rede numa tentativa: não aborta — segue tentando até o deadline.
            Err(e) => {
                eprintln!("(aviso de rede, tentando de novo: {e})");
                continue;
            }
        }
    }
}

/// `schematize logout` — apaga a sessão local.
fn logout_cmd() {
    if account::is_logged_in() {
        account::logout();
        println!("Sessão encerrada. Você não está mais logado.");
    } else {
        println!("Você já não estava logado.");
    }
}

/// `schematize whoami` — mostra o subject da conta logada (ou avisa que não há sessão).
fn whoami_cmd() {
    match account::account_sub() {
        Some(sub) => println!("Logado como: {sub}"),
        None => println!("Você não está logado. Rode `schematize login`."),
    }
}

/// `schematize skills <sub>` — dispatcher da gestão de skills (a feature).
fn skills_cmd(sub: SkillsCmd) -> Result<(), String> {
    match sub {
        SkillsCmd::Install { names, all, with_recommended } => {
            skills_install(&names, all, with_recommended)
        }
        SkillsCmd::Update { names, all } => skills_update(&names, all),
        SkillsCmd::List => skills_list(),
        SkillsCmd::Remove { name } => skills_remove(&name),
        SkillsCmd::New { slug, name, desc, force } => skills_new(&slug, name, desc, force),
        SkillsCmd::Edit { slug, list, file, set_from } => skills_edit(&slug, list, file, set_from),
        SkillsCmd::Fork { slug } => skills_fork(&slug),
        SkillsCmd::Compare { slug } => skills_compare(&slug),
    }
}

/// `schematize skills fork <slug>` — força o fork de uma skill oficial (guarda a base no stash).
fn skills_fork(slug: &str) -> Result<(), String> {
    if !skills::is_official(slug) {
        println!("skill {slug} não é oficial (do catálogo) — ela já edita livremente, sem fork.");
        return Ok(());
    }
    skills::fork(slug)?;
    println!("skill {slug} forkada: a pasta ativa é editável e a base oficial ficou guardada no stash.");
    println!("compare depois com: schematize skills compare {slug}");
    Ok(())
}

/// `schematize skills compare <slug>` — mostra o diff do fork ativo vs a nova oficial (latest).
fn skills_compare(slug: &str) -> Result<(), String> {
    let cmp = skills::compare_update(slug)?;
    println!("Comparando fork de {slug}: base v{} → nova oficial v{}", cmp.base_version, cmp.new_version);
    if cmp.files.is_empty() {
        println!("  (nenhum arquivo — nada a comparar)");
    }
    for f in &cmp.files {
        println!("  {:<10} {}", f.status, f.path);
    }
    if !cmp.diff_text.trim().is_empty() {
        println!("\n--- diff unificado (fork ativo → nova oficial) ---");
        print!("{}", cmp.diff_text);
    }
    Ok(())
}

/// `schematize skills new <slug>` — scaffolda o piso mínimo válido de uma skill nova.
fn skills_new(slug: &str, name: Option<String>, desc: Option<String>, force: bool) -> Result<(), String> {
    let name = name.unwrap_or_else(|| slug.to_string());
    let desc = desc.unwrap_or_default();
    let dest = if force {
        skilledit::scaffold_force(slug, &name, &desc)?
    } else {
        skilledit::scaffold(slug, &name, &desc)?
    };
    println!("{}", tf("skilledit.created", &[("path", &dest.display().to_string())]));
    Ok(())
}

/// `schematize skills edit <slug>` — lista os arquivos, imprime um, ou o grava de um arquivo fonte.
fn skills_edit(slug: &str, list: bool, file: Option<String>, set_from: Option<String>) -> Result<(), String> {
    // Com --file: escreve (se --set-from) ou imprime o conteúdo.
    if let Some(rel) = file {
        if let Some(src) = set_from {
            let content = std::fs::read_to_string(&src).map_err(|e| format!("falha ao ler {src}: {e}"))?;
            skilledit::write_file(slug, &rel, &content)?;
            println!("{}", tf("skilledit.wrote", &[("file", &rel)]));
            return Ok(());
        }
        let content = skilledit::read_file(slug, &rel)?;
        print!("{content}");
        return Ok(());
    }
    // Sem --file: lista (o default quando nada mais é passado). `--list` é o mesmo comportamento.
    let _ = list;
    let files = skilledit::list_files(slug)?;
    println!("{}", tf("skilledit.files_header", &[("slug", slug)]));
    for f in files {
        println!("  {f}");
    }
    Ok(())
}

/// Instala skills (ou todas com --all) e, opcionalmente, as recomendadas.
fn skills_install(names: &[String], all: bool, with_recommended: bool) -> Result<(), String> {
    let cat = registry::catalog();
    let selected = resolve(&cat, names, all);
    for it in &selected {
        match skills::install(it) {
            Ok(v) => println!("✓ {}", tf("skills.installed_ok", &[("name", &it.slug), ("v", &v)])),
            Err(e) => eprintln!("✗ {}: {e}", it.slug),
        }
    }
    // Recomendações (skill BASE complementar). Nunca instala de surpresa:
    // com --all já vem tudo; senão sugere, e só instala com --with-recommended.
    if !all {
        let mut suggested: Vec<String> = Vec::new();
        for it in &selected {
            for rec in &it.recommends {
                let already = registry::find(&cat, rec)
                    .map(|r| skills::installed_version(&r).is_some())
                    .unwrap_or(false);
                let in_batch = selected.iter().any(|s| &s.slug == rec);
                if !already && !in_batch && !suggested.contains(rec) {
                    suggested.push(rec.clone());
                }
            }
        }
        if !suggested.is_empty() {
            if with_recommended {
                for rec in &suggested {
                    if let Some(r) = registry::find(&cat, rec) {
                        match skills::install(&r) {
                            Ok(v) => println!("✓ {}", tf("skills.installed_ok", &[("name", &r.slug), ("v", &v)])),
                            Err(e) => eprintln!("✗ {}: {e}", r.slug),
                        }
                    }
                }
            } else {
                println!("{}", tf("skills.recommends_hint", &[("list", &suggested.join(", "))]));
            }
        }
    }
    Ok(())
}

/// Atualiza skills instaladas pro latest (todas se não passar nome/--all). Skills FORKADAS
/// não são sobrescritas — `skills::update` recusa e aponta o caminho comparar/mesclar.
fn skills_update(names: &[String], all: bool) -> Result<(), String> {
    let cat = registry::catalog();
    for it in resolve(&cat, names, all) {
        match skills::update(&it) {
            Ok(v) => println!("✓ {}", tf("skills.updated", &[("name", &it.slug), ("v", &v)])),
            Err(e) => eprintln!("✗ {}: {e}", it.slug),
        }
    }
    Ok(())
}

/// Lista skills: instaladas vs última disponível. Se houver rede, anexa a nota do marketplace
/// (uma única request pra todas as linhas) — offline não trava: o mapa vem vazio e a nota some.
fn skills_list() -> Result<(), String> {
    let st = skills::load_state();
    let ratings = market::market_ratings_all(); // vazio se offline; não bloqueia a listagem
    println!("{}", t("skills.header"));
    for it in &registry::catalog() {
        let line = skills::status_line(it, &st, true);
        let nota = market::format_rating(ratings.get(&it.slug).copied());
        if nota.is_empty() {
            println!("  {line}");
        } else {
            println!("  {line}  {nota}");
        }
    }
    Ok(())
}

/// Remove uma skill instalada.
fn skills_remove(name: &str) -> Result<(), String> {
    let cat = registry::catalog();
    match registry::find(&cat, name) {
        Some(it) => skills::remove(&it).map(|_| println!("{}", tf("skills.removed", &[("name", &it.slug)]))),
        None => Err(tf("skills.unknown", &[("name", name)])),
    }
}

/// Resolve a fonte do schema pro `db sql|graph`: --from <json> | --sqlite | --postgres.
fn db_source(
    from: Option<String>,
    sqlite: Option<String>,
    postgres: Option<String>,
) -> Result<database::Schema, String> {
    if let Some(f) = from {
        let s = std::fs::read_to_string(&f).map_err(|e| format!("ler {f}: {e}"))?;
        return serde_json::from_str(&s).map_err(|e| format!("schema JSON inválido em {f}: {e}"));
    }
    if let Some(p) = sqlite {
        return database::introspect_sqlite(std::path::Path::new(&p));
    }
    if let Some(c) = postgres {
        return database::introspect_postgres(&c);
    }
    Err("informe a fonte do schema: --from <schema.json> | --sqlite <arquivo> | --postgres <conn>".into())
}

/// Imprime o resumo humano de um schema (tabelas, nº de colunas/FKs/índices + totais).
fn db_print_summary(schema: &database::Schema) {
    let mut cols = 0usize;
    let mut fks = 0usize;
    for t in &schema.tables {
        cols += t.columns.len();
        fks += t.fks.len();
        let pk: Vec<&str> = t.columns.iter().filter(|c| c.pk).map(|c| c.name.as_str()).collect();
        println!(
            "  {} — {} coluna(s), {} FK(s), {} índice(s){}",
            t.name,
            t.columns.len(),
            t.fks.len(),
            t.indexes.len(),
            if pk.is_empty() { String::new() } else { format!("; PK: {}", pk.join(", ")) }
        );
    }
    println!("total: {} tabela(s), {cols} coluna(s), {fks} FK(s).", schema.tables.len());
}

/// `schematize db <sub>` — backend do database builder (introspect | sql | graph).
fn db_cmd(sub: DbCmd) -> Result<(), String> {
    match sub {
        DbCmd::Introspect { sqlite, postgres, json } => {
            let schema = db_source(None, sqlite, postgres)?;
            println!("Schema ({} tabela(s)):", schema.tables.len());
            db_print_summary(&schema);
            if json {
                let js = serde_json::to_string_pretty(&schema).map_err(|e| e.to_string())?;
                println!("\n--- schema.json ---\n{js}");
            }
            Ok(())
        }
        DbCmd::Sql { from, sqlite, postgres, migration } => {
            let schema = db_source(from, sqlite, postgres)?;
            if migration {
                print!("{}", database::to_migration(&schema));
            } else {
                print!("{}", database::to_sql(&schema));
            }
            Ok(())
        }
        DbCmd::Graph { from, sqlite, postgres } => {
            let schema = db_source(from, sqlite, postgres)?;
            let (nodes, edges) = database::to_graph(&schema);
            println!("nós ({}):", nodes.len());
            for n in &nodes {
                println!("  {}", n.id);
            }
            println!("arestas ({}):", edges.len());
            for e in &edges {
                match &e.label {
                    Some(l) => println!("  {} -> {} ({l})", e.from, e.to),
                    None => println!("  {} -> {}", e.from, e.to),
                }
            }
            Ok(())
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let r: Result<(), String> = match cli.cmd {
        // Feature SKILLS, agrupada. O app é uma coisa; skills são uma funcionalidade.
        Cmd::Skills { sub } => skills_cmd(sub),
        // Aliases ocultos de compat (mesma lógica que o subcomando `skills`).
        Cmd::Install { names, all, with_recommended } => {
            skills_install(&names, all, with_recommended)
        }
        Cmd::Update { names, all } => skills_update(&names, all),
        Cmd::List => skills_list(),
        Cmd::Remove { name } => skills_remove(&name),
        Cmd::Status => {
            status::run();
            Ok(())
        }
        Cmd::Agents { json, split } => agents_cmd(json, split),
        Cmd::Doctor { fix } => {
            doctor::run(fix);
            Ok(())
        }
        Cmd::Debug { collect, out, stdout, online } => debug_cmd(collect, out, stdout, online),
        Cmd::Upgrade { force } => upgrade::run(force),
        Cmd::News => {
            news::show();
            Ok(())
        }
        Cmd::Notifications => {
            notifications_cmd();
            Ok(())
        }
        Cmd::Blog => links::open("blog"),
        Cmd::Open { target } => links::open(&target),
        Cmd::Lang { code, list } => lang_cmd(code, list),
        Cmd::Overdev { sub } => match sub {
            Over::Enable => overdev::enable(),
            Over::Disable => overdev::disable(),
            Over::Start { objetivo, max } => overdev::start(&objetivo.join(" "), max),
            Over::Split { k, dispatch, force } => overdev_split(k, dispatch, force),
            Over::Check => {
                overdev::check();
                Ok(())
            }
            Over::Guard => {
                overdev::guard();
                Ok(())
            }
            Over::Status => {
                overdev::status();
                Ok(())
            }
            Over::Hold { texto } => overdev::hold(&texto.join(" ")),
            Over::Park { item, pergunta } => overdev::park(&item, &pergunta.join(" ")),
            Over::Human { texto, done } => {
                let t = texto.join(" ");
                let sub = if t.trim().is_empty() { None } else { Some(t.as_str()) };
                overdev::human_done(sub, done)
            }
            Over::Note { texto, kind } => overdev::note(&kind, &texto.join(" ")),
            Over::Stop => overdev::stop(),
            Over::Run { max, yes } => overdev_run(max, yes),
            Over::Snapshot => overdev_snapshot(),
            Over::History { limit } => overdev_history(limit),
            Over::Restore { id } => overdev_restore(id),
            Over::Load => overdev_agent_cmd(overdev::load_cmd()),
            Over::Index => overdev_agent_cmd(overdev::index_cmd()),
            Over::Log => {
                overdev_log();
                Ok(())
            }
        },
        Cmd::GitLog { limit } => {
            git_log(limit);
            Ok(())
        }
        Cmd::Panel => panel::open(),
        Cmd::Graph { sub } => match sub {
            GraphCmd::Obsidian { out } => panel::export_obsidian(out),
        },
        Cmd::Db { sub } => db_cmd(sub),
        Cmd::Check { notify } => {
            agent::run_once(notify);
            Ok(())
        }
        Cmd::Agent => {
            agent::run_loop();
            Ok(())
        }
        Cmd::Autostart { sub } => match sub {
            Auto::Enable => autostart::enable(&util::self_exe()),
            Auto::Disable => autostart::disable(),
        },
        Cmd::Env { sub } => match sub {
            EnvCmd::List => {
                environments::list();
                Ok(())
            }
            EnvCmd::Install { lang, method, dry_run, yes } => {
                environments::install(&lang, method, dry_run, yes)
            }
            EnvCmd::Remove { lang, method, dry_run } => {
                environments::remove(&lang, method, dry_run)
            }
        },
        Cmd::Gui => {
            // Mesma aplicação, outra face. A face gráfica DEFAULT é o binário
            // `schematize-gui` (Slint), instalado à parte pelo install.sh; executa-o.
            // Se ele não estiver no PATH (ex.: build do Slint falhou), cai na GUI egui
            // EMBUTIDA (fallback) — a virada é segura: nunca fica sem janela.
            match std::process::Command::new("schematize-gui").status() {
                Ok(st) if st.success() => Ok(()),
                Ok(st) => Err(format!("schematize-gui saiu com {st}")),
                Err(_) => {
                    #[cfg(feature = "gui")]
                    {
                        schematize::gui::run().map_err(|e| e.to_string())
                    }
                    #[cfg(not(feature = "gui"))]
                    {
                        Err("GUI indisponível — reinstale (o install.sh instala o schematize-gui).".to_string())
                    }
                }
            }
        }
        Cmd::Archive => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            schematize::archive::sync(&cwd).map(|msg| println!("{msg}"))
        }
        Cmd::Ssh { sub } => ssh_cmd(sub),
        Cmd::Projects { sub } => projects_cmd(sub),
        Cmd::Login => login_cmd(),
        Cmd::Logout => {
            logout_cmd();
            Ok(())
        }
        Cmd::Whoami => {
            whoami_cmd();
            Ok(())
        }
    };
    if let Err(e) = r {
        eprintln!("{}", tf("err.prefix", &[("e", &e)]));
        std::process::exit(1);
    }
}
