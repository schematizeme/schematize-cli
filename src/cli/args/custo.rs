//! `schematize cost …` — a contabilidade de custo (tokens + US$) do projeto.
//!
//! **Onde:** referenciado pelo `Cmd::Cost` em `args/mod.rs`; o trabalho está em `cli/custo.rs`.

use clap::{Subcommand, ValueEnum};

/// Eixo de `cost show`.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Eixo {
    Modelo,
    Dia,
    Semana,
    Task,
    Sessao,
    Agente,
}

#[derive(Subcommand)]
pub(crate) enum CostCmd {
    /// Read the Claude Code transcripts and append the new responses to the ledger (idempotent).
    Sync {
        /// Project folder or name (default: the project of the current directory).
        #[arg(long)]
        projeto: Option<String>,
    },
    /// Regenerate custos.json and custos.md from the ledger.
    Report {
        #[arg(long)]
        projeto: Option<String>,
        /// Also print custos.json to stdout.
        #[arg(long, conflicts_with = "md")]
        json: bool,
        /// Also print custos.md to stdout.
        #[arg(long)]
        md: bool,
    },
    /// Show a table straight from the ledger (default: by model).
    Show {
        #[arg(long)]
        projeto: Option<String>,
        #[arg(long, value_enum, default_value = "modelo")]
        por: Eixo,
    },
    /// Attribute the usage from now on to a task, until the next tag or `untag`.
    Tag {
        task_id: String,
        /// Human-readable task name.
        #[arg(long)]
        nome: Option<String>,
        #[arg(long)]
        projeto: Option<String>,
    },
    /// Close the active task tag.
    Untag {
        #[arg(long)]
        projeto: Option<String>,
    },
    /// List the effective price table; with --check, validate it (exit 1 on problems).
    Pricing {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        projeto: Option<String>,
    },
}
