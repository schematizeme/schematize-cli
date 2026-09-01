//! Superfície do OVERDEV: o laço de desenvolvimento contínuo, a caixa de entrada, o grafo
//! e os projetos. É o maior dos quatro grupos, e o que mais cresce.
//!
//! **Onde:** referenciado pelo `Cmd` em `args/mod.rs`, que e a raiz da arvore do clap.
//!
//! **Por que este arquivo existe:** o `args.rs` chegou a 780 linhas (512 uteis),
//! acima do teto de 750 da casa — cresceu com os subcomandos de VPS. O corte e por
//! DOMINIO, nao por tamanho: cada arquivo e uma superficie que a pessoa reconhece.
//! A superficie da CLI e identica ao que era, e ha teste provando isso
//! (`superficie_da_cli_nao_mudou`, contra `tests/superficie-cli.txt`).

use clap::Subcommand;

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
    Add { texto: Vec<String> },
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

#[derive(Subcommand)]
pub(crate) enum Auto {
    /// Enable and start the agent (systemd --user + XDG autostart).
    Enable,
    /// Disable and remove the autostart.
    Disable,
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
