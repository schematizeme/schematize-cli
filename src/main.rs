//! schematize — gerenciador do ecossistema para o Claude (skills, overdev e mais).
//! O quê: CLI multi-idioma que instala/versiona skills, roda o overdev, diagnostica
//! o ambiente (doctor), atualiza a si mesmo (upgrade), mostra status e o blog.
//! Onde: ponto de entrada; despacha pros módulos da lib `schematize`.

use clap::{Parser, Subcommand};
use schematize::agentrun::AgentRunner;
use schematize::i18n::{t, tf};
use schematize::{
    agent, agentrun, autostart, debug, doctor, environments, i18n, links, news, overdev, panel,
    registry, skilledit, skills, sshkeys, status, upgrade, util,
};
use std::io::{self, BufRead, Write};

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
    /// Diagnose the environment (add --fix to repair what's safe).
    Doctor {
        #[arg(long)]
        fix: bool,
    },
    /// Debug the updater/versioning (versão, rate limit, exe, shadows, catálogo, log).
    Debug,
    /// Update schematize itself — atualiza o próprio schematize (o APP, o binário), não as skills.
    Upgrade {
        #[arg(long)]
        force: bool,
    },
    /// Show the latest posts from blog.schematize.net.
    News,
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
    /// Open the auxiliary HTML panel (overdev + index graph) in the browser.
    Panel,
    /// Graph tools (export the index graph, Obsidian-style).
    Graph {
        #[command(subcommand)]
        sub: GraphCmd,
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
    /// Language environments: install a runtime + common tools (docker|mise|distro|official).
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
    Export {
        name: String,
        #[arg(long)]
        copy: bool,
    },
    /// Remove a key pair (private + public) with confirmation.
    Rm { name: String },
    /// Add an existing PUBLIC key to your GitHub account (gh must be authenticated).
    Github { name: String },
}

#[derive(Subcommand)]
enum EnvCmd {
    /// List languages, methods available on this machine, and install status.
    List,
    /// Install a language runtime + tools via a chosen method.
    Install {
        lang: String,
        /// docker | mise | distro | official (required; no default — deny-by-default).
        #[arg(long)]
        method: Option<String>,
        /// Print everything and execute nothing.
        #[arg(long)]
        dry_run: bool,
        /// Skip the interactive confirmation (run without asking).
        #[arg(long)]
        yes: bool,
    },
    /// Remove a language environment (auto-detects the method if omitted).
    Remove {
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
    /// Attach a human note (correction prompt / per-task point) in .overdev/NOTAS.md.
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
}

fn resolve(cat: &[registry::Item], names: &[String], all: bool) -> Vec<registry::Item> {
    if all || names.is_empty() {
        cat.to_vec()
    } else {
        names.iter().filter_map(|n| registry::find(cat, n)).collect()
    }
}

/// `schematize lang [code] [--list]`.
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
        SshCmd::Export { name, copy } => {
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
    }
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

/// Atualiza skills instaladas pro latest (todas se não passar nome/--all).
fn skills_update(names: &[String], all: bool) -> Result<(), String> {
    let cat = registry::catalog();
    for it in resolve(&cat, names, all) {
        match skills::install(&it) {
            Ok(v) => println!("✓ {}", tf("skills.updated", &[("name", &it.slug), ("v", &v)])),
            Err(e) => eprintln!("✗ {}: {e}", it.slug),
        }
    }
    Ok(())
}

/// Lista skills: instaladas vs última disponível.
fn skills_list() -> Result<(), String> {
    let st = skills::load_state();
    println!("{}", t("skills.header"));
    for it in &registry::catalog() {
        println!("  {}", skills::status_line(it, &st, true));
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
        Cmd::Doctor { fix } => {
            doctor::run(fix);
            Ok(())
        }
        Cmd::Debug => {
            debug::run();
            Ok(())
        }
        Cmd::Upgrade { force } => upgrade::run(force),
        Cmd::News => {
            news::show();
            Ok(())
        }
        Cmd::Blog => links::open("blog"),
        Cmd::Open { target } => links::open(&target),
        Cmd::Lang { code, list } => lang_cmd(code, list),
        Cmd::Overdev { sub } => match sub {
            Over::Enable => overdev::enable(),
            Over::Disable => overdev::disable(),
            Over::Start { objetivo, max } => overdev::start(&objetivo.join(" "), max),
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
        },
        Cmd::Panel => panel::open(),
        Cmd::Graph { sub } => match sub {
            GraphCmd::Obsidian { out } => panel::export_obsidian(out),
        },
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
        Cmd::Ssh { sub } => ssh_cmd(sub),
    };
    if let Err(e) = r {
        eprintln!("{}", tf("err.prefix", &[("e", &e)]));
        std::process::exit(1);
    }
}
