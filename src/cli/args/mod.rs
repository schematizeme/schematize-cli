//! Definição da linha de comando (clap): o `Cli`, a raiz `Cmd`, e os enums de subcomando
//! divididos por DOMÍNIO nos submódulos. É o CONTRATO da CLI — o que o usuário pode pedir.
//!
//! # A divisão
//!
//! | módulo | superfície |
//! |---|---|
//! | [`remoto`] | VPS e o servidor MCP que o expõe ao agente |
//! | [`maquina`] | SSH, discos, ambientes, banco, contas git |
//! | [`skills`] | instalar/atualizar/remover/listar skills |
//! | [`overdev`] | o laço contínuo, a caixa, o grafo, os projetos |
//!
//! Os enums são **re-exportados aqui** (`pub(crate) use`), então `args::VpsCmd` continua
//! resolvendo como sempre: nenhum `use` do resto do crate mudou por causa do corte.
//!
//! # Por que foi partido
//!
//! O arquivo chegou a 780 linhas (512 úteis), acima do teto de 750 da casa — cresceu com os
//! subcomandos de VPS. O corte é por domínio, não por tamanho: cada arquivo é uma superfície
//! que a pessoa reconhece, e o `Cmd` continua sendo o único lugar onde a árvore se vê
//! inteira.
//!
//! # A superfície NÃO mudou
//!
//! Cortar a definição da CLI é mexer no que scripts, hooks e documentação de outras pessoas
//! digitam. Por isso o corte foi feito **depois** de congelar a árvore inteira em
//! `tests/superficie-cli.txt` — 127 comandos com seus aliases, flags e obrigatoriedade — e
//! `superficie_da_cli_nao_mudou` compara a cada `cargo test`. O corte é provado, não
//! confiado.

pub(crate) mod maquina;
pub(crate) mod overdev;
pub(crate) mod remoto;
pub(crate) mod skills;

pub(crate) use maquina::{DbCmd, DiscoCmd, EnvCmd, GitCmd, SshCmd};
pub(crate) use overdev::{Auto, CaixaCmd, GraphCmd, Over, ProjectsCmd};
pub(crate) use remoto::{McpCmd, VpsCmd};
pub(crate) use skills::SkillsCmd;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "schematize",
    version,
    about = "Ecosystem manager for Claude — skills, overdev, and more (Linux-first)."
)]
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
    /// VPS: registro de hosts + execução remota AUDITADA (o agente nunca vê a chave).
    /// A política do cliente é UX; a fronteira é o forced command no servidor (ADR-0005).
    Vps {
        #[command(subcommand)]
        sub: VpsCmd,
    },
    /// MCP: expõe o gestor de VPS ao agente como tools tipadas (`mcp__schematize-vps__*`).
    Mcp {
        #[command(subcommand)]
        sub: McpCmd,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Serializa a ÁRVORE INTEIRA de comandos num texto determinístico.
    ///
    /// **O quê:** para cada comando e subcomando, em ordem alfabética — o caminho, os
    /// aliases, se é oculto, e cada argumento com nome longo/curto, se é obrigatório, se
    /// recebe valor e qual o default. Nada de texto de ajuda: descrição muda com revisão de
    /// prosa e não é contrato; o que a pessoa DIGITA é.
    ///
    /// **Onde:** [`superficie_da_cli_nao_mudou`], contra um snapshot commitado.
    fn superficie(c: &clap::Command, caminho: &str, out: &mut Vec<String>) {
        let nome = if caminho.is_empty() {
            c.get_name().to_string()
        } else {
            format!("{caminho} {}", c.get_name())
        };
        let mut aliases: Vec<_> = c.get_all_aliases().collect();
        aliases.sort_unstable();
        out.push(format!(
            "CMD {nome}{}{}",
            if aliases.is_empty() {
                String::new()
            } else {
                format!(" aliases=[{}]", aliases.join(","))
            },
            if c.is_hide_set() { " (oculto)" } else { "" }
        ));
        // `SOBRE` é a descrição — vai pro arquivo (é dela que o índice de funcionalidades
        // se alimenta) mas FICA DE FORA da comparação: prosa muda em revisão de texto e não
        // é contrato. Quem quebra script é a linha `CMD`/`ARG`, não o `about`.
        if let Some(sobre) = c.get_about() {
            let t = sobre.to_string();
            out.push(format!("  SOBRE {}", t.lines().next().unwrap_or("").trim()));
        }

        let mut args: Vec<String> = c
            .get_arguments()
            .map(|a| {
                let longo = a.get_long().map(|l| format!("--{l}")).unwrap_or_default();
                let curto = a.get_short().map(|s| format!(" -{s}")).unwrap_or_default();
                let val = if a.get_num_args().map(|n| n.takes_values()).unwrap_or(false) {
                    " <valor>"
                } else {
                    ""
                };
                let obrig = if a.is_required_set() { " OBRIGATORIO" } else { "" };
                format!("  ARG {:<20} {longo}{curto}{val}{obrig}", a.get_id().to_string())
            })
            .collect();
        args.sort();
        out.extend(args);

        let mut subs: Vec<&clap::Command> = c.get_subcommands().collect();
        subs.sort_by_key(|s| s.get_name());
        for s in subs {
            superficie(s, &nome, out);
        }
    }

    /// A superfície da CLI é CONTRATO com quem escreveu script, hook e documentação.
    ///
    /// **Onde:** roda a cada `cargo test`, e existe pra que refatorar este módulo seja
    /// seguro — o `args.rs` passou de 780 linhas e precisou ser partido em submódulos por
    /// domínio, e sem esta prova o corte seria confiança, não verificação.
    ///
    /// **Se este teste falhar** e a mudança for INTENCIONAL (comando novo, flag nova),
    /// regenere com `SCHEMATIZE_REGRAVA_SUPERFICIE=1 cargo test superficie_da_cli` e leia o
    /// diff **linha por linha** antes de commitar: cada linha some é um script de alguém
    /// quebrando. Se foi acidente de refatoração, o teste acabou de fazer o trabalho dele.
    #[test]
    fn superficie_da_cli_nao_mudou() {
        let mut linhas = Vec::new();
        superficie(&Cli::command(), "", &mut linhas);
        let atual = linhas.join("\n") + "\n";

        let snap = std::path::Path::new("tests/superficie-cli.txt");
        if std::env::var_os("SCHEMATIZE_REGRAVA_SUPERFICIE").is_some() {
            std::fs::write(snap, &atual).expect("gravar o snapshot");
            return;
        }
        let esperado = std::fs::read_to_string(snap).expect(
            "tests/superficie-cli.txt ausente — gere com \
             SCHEMATIZE_REGRAVA_SUPERFICIE=1 cargo test superficie_da_cli",
        );
        // A ASSERÇÃO é só sobre o contrato: `CMD` e `ARG`. As linhas `SOBRE` viajam no
        // arquivo pra alimentar o índice de funcionalidades, e mudam livremente com revisão
        // de prosa — descrição não quebra o script de ninguém.
        let contrato = |t: &str| -> Vec<String> {
            t.lines().filter(|l| !l.trim_start().starts_with("SOBRE ")).map(String::from).collect()
        };
        if contrato(&atual) == contrato(&esperado) {
            // Só a prosa mudou: regrava sem reprovar.
            if atual != esperado {
                std::fs::write(snap, &atual).expect("regravar a prosa do snapshot");
            }
            return;
        }
        // Diff legível: a primeira divergência é o que a pessoa precisa ver.
        let (a, e) = (contrato(&atual), contrato(&esperado));
        let sumiram: Vec<_> = e.iter().filter(|l| !a.contains(l)).collect();
        let surgiram: Vec<_> = a.iter().filter(|l| !e.contains(l)).collect();
        panic!(
            "a superfície da CLI MUDOU.\n\nsumiram ({}) — cada uma é um script de alguém \
             quebrando:\n  {}\n\nsurgiram ({}):\n  {}\n\nSe foi intencional: \
             SCHEMATIZE_REGRAVA_SUPERFICIE=1 cargo test superficie_da_cli",
            sumiram.len(),
            sumiram.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n  "),
            surgiram.len(),
            surgiram.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n  "),
        );
    }
}
