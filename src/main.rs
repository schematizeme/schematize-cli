//! schematize — gerenciador do ecossistema para o Claude (skills, overdev e mais).
//! O quê: CLI multi-idioma que instala/versiona skills, roda o overdev, diagnostica
//! o ambiente (doctor), atualiza a si mesmo (upgrade), mostra status e o blog.
//! Onde: ponto de entrada; despacha pros módulos da lib `schematize`.

use clap::{Parser, Subcommand};
use schematize::i18n::{t, tf};
use schematize::{
    agent, autostart, debug, doctor, environments, i18n, links, news, overdev, panel, registry,
    skills, status, upgrade, util,
};

#[derive(Parser)]
#[command(name = "schematize", version, about = "Ecosystem manager for Claude — skills, overdev, and more (Linux-first).")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
    /// Overview dashboard: versions, agent, overdev, language, links.
    Status,
    /// Diagnose the environment (add --fix to repair what's safe).
    Doctor {
        #[arg(long)]
        fix: bool,
    },
    /// Debug the updater/versioning (versão, rate limit, exe, shadows, catálogo, log).
    Debug,
    /// Update schematize itself to the latest version.
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
    /// End the run (hooks become inert again).
    Stop,
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

fn main() {
    let cli = Cli::parse();
    let r: Result<(), String> = match cli.cmd {
        Cmd::Install { names, all, with_recommended } => {
            let cat = registry::catalog();
            let selected = resolve(&cat, &names, all);
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
        Cmd::Update { names, all } => {
            let cat = registry::catalog();
            for it in resolve(&cat, &names, all) {
                match skills::install(&it) {
                    Ok(v) => println!("✓ {}", tf("skills.updated", &[("name", &it.slug), ("v", &v)])),
                    Err(e) => eprintln!("✗ {}: {e}", it.slug),
                }
            }
            Ok(())
        }
        Cmd::List => {
            let st = skills::load_state();
            println!("{}", t("skills.header"));
            for it in &registry::catalog() {
                println!("  {}", skills::status_line(it, &st, true));
            }
            Ok(())
        }
        Cmd::Remove { name } => {
            let cat = registry::catalog();
            match registry::find(&cat, &name) {
                Some(it) => skills::remove(&it).map(|_| println!("{}", tf("skills.removed", &[("name", &it.slug)]))),
                None => Err(tf("skills.unknown", &[("name", &name)])),
            }
        }
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
            Over::Stop => overdev::stop(),
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
    };
    if let Err(e) = r {
        eprintln!("{}", tf("err.prefix", &[("e", &e)]));
        std::process::exit(1);
    }
}
