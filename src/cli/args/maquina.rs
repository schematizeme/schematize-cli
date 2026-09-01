//! Superfície da MÁQUINA LOCAL: chaves SSH, discos, ambientes, banco e contas git.
//! O que o app faz no computador de quem o usa, antes de qualquer rede.
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
