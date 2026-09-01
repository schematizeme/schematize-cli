//! Superfície de SKILLS — instalar, atualizar, remover, listar. É a feature mais antiga do
//! app e a que mais tem alias de compatibilidade pendurado no `Cmd`.
//!
//! **Onde:** referenciado pelo `Cmd` em `args/mod.rs`, que e a raiz da arvore do clap.
//!
//! **Por que este arquivo existe:** o `args.rs` chegou a 780 linhas (512 uteis),
//! acima do teto de 750 da casa — cresceu com os subcomandos de VPS. O corte e por
//! DOMINIO, nao por tamanho: cada arquivo e uma superficie que a pessoa reconhece.
//! A superficie da CLI e identica ao que era, e ha teste provando isso
//! (`superficie_da_cli_nao_mudou`, contra `tests/superficie-cli.txt`).

use clap::Subcommand;

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
