//! schematize — gerenciador do ecossistema (skills + overdev). Linux-first.
//! O quê: CLI que instala/versiona as skills do catálogo e roda o modo overdev
//! (dev contínuo à prova de parada, sem travar pra perguntar).
//! Onde: ponto de entrada; despacha pros módulos skills/overdev.

mod overdev;
mod registry;
mod settings;
mod skills;
mod util;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "schematize", version, about = "Instala/versiona as skills schematize e roda o overdev (Linux-first).")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Instala uma ou mais skills (ou todas com --all) a partir do release latest.
    Install {
        names: Vec<String>,
        #[arg(long)]
        all: bool,
    },
    /// Atualiza skills instaladas pro latest (todas se nenhum nome/--all).
    Update {
        names: Vec<String>,
        #[arg(long)]
        all: bool,
    },
    /// Lista skills: instalada vs última disponível.
    List,
    /// Remove uma skill instalada.
    Remove { name: String },
    /// Modo overdev — dev contínuo até o checklist fechar.
    Overdev {
        #[command(subcommand)]
        sub: Over,
    },
}

#[derive(Subcommand)]
enum Over {
    /// Registra os hooks (Stop + veto de AskUserQuestion) no settings.json.
    Enable,
    /// Remove os hooks do overdev do settings.json.
    Disable,
    /// Inicia um run no diretório atual: `schematize overdev start "<objetivo>"`.
    Start {
        objetivo: Vec<String>,
        #[arg(long)]
        max: Option<u64>,
    },
    /// (hook Stop) rejeita a parada enquanto houver item aberto.
    Check,
    /// (hook PreToolUse) veta AskUserQuestion em overdev.
    Guard,
    /// Mostra o estado do run.
    Status,
    /// Marca o primeiro item aberto que casa o texto como on-hold.
    Hold { texto: Vec<String> },
    /// Parkeia uma pergunta (registra no txt da base) e marca o item on-hold.
    Park { item: String, pergunta: Vec<String> },
    /// Encerra o run (hooks voltam a ser inertes).
    Stop,
}

fn resolve(names: &[String], all: bool) -> Vec<&'static registry::Item> {
    if all || names.is_empty() {
        registry::ITEMS.iter().collect()
    } else {
        names.iter().filter_map(|n| registry::find(n)).collect()
    }
}

fn main() {
    let cli = Cli::parse();
    let r: Result<(), String> = match cli.cmd {
        Cmd::Install { names, all } => {
            for it in resolve(&names, all) {
                match skills::install(it) {
                    Ok(v) => println!("✓ {} instalada v{v}", it.slug),
                    Err(e) => eprintln!("✗ {}: {e}", it.slug),
                }
            }
            Ok(())
        }
        Cmd::Update { names, all } => {
            for it in resolve(&names, all) {
                match skills::install(it) {
                    Ok(v) => println!("✓ {} → v{v}", it.slug),
                    Err(e) => eprintln!("✗ {}: {e}", it.slug),
                }
            }
            Ok(())
        }
        Cmd::List => {
            let st = skills::load_state();
            println!("catálogo schematize (instalada vs latest):");
            for it in registry::ITEMS {
                println!("  {}", skills::status_line(it, &st, true));
            }
            Ok(())
        }
        Cmd::Remove { name } => match registry::find(&name) {
            Some(it) => skills::remove(it).map(|_| println!("removida: {}", it.slug)),
            None => Err(format!("skill desconhecida: {name}")),
        },
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
    };
    if let Err(e) = r {
        eprintln!("erro: {e}");
        std::process::exit(1);
    }
}
