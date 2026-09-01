//! Superfície de ACESSO REMOTO: o gestor de VPS e o servidor MCP que o expõe ao agente.
//! Os dois andam juntos — toda tool do MCP corresponde a um verbo do `vps`, e a política
//! que decide o veredito é a mesma.
//!
//! **Onde:** referenciado pelo `Cmd` em `args/mod.rs`, que e a raiz da arvore do clap.
//!
//! **Por que este arquivo existe:** o `args.rs` chegou a 780 linhas (512 uteis),
//! acima do teto de 750 da casa — cresceu com os subcomandos de VPS. O corte e por
//! DOMINIO, nao por tamanho: cada arquivo e uma superficie que a pessoa reconhece.
//! A superficie da CLI e identica ao que era, e ha teste provando isso
//! (`superficie_da_cli_nao_mudou`, contra `tests/superficie-cli.txt`).

use clap::Subcommand;

/// Gestão de VPS — o "Termius embutido": hosts registrados, execução auditada e a política.
#[derive(Subcommand)]
pub(crate) enum VpsCmd {
    /// Registra um host. Nasce em `prd` + `readonly` (o mais restritivo) — ajuste com `policy`.
    Add {
        /// Nome curto do host (letras, números, '.', '_' ou '-').
        alias: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        user: String,
        /// Nome da chave gerenciada em ~/.ssh (veja `schematize ssh list`).
        #[arg(long)]
        key: String,
        #[arg(long, default_value_t = 22)]
        port: u16,
        /// Ambiente: dev | hml | prd. Qualquer outra coisa vira `prd` (falha fechada).
        #[arg(long, default_value = "prd")]
        env: String,
        /// ProxyJump explícito (`user@bastion`) — o ~/.ssh/config NÃO é lido.
        #[arg(long)]
        jump: Option<String>,
    },
    /// Lista os hosts registrados, com ambiente, modo e se têm fronteira server-side.
    List,
    /// Mostra a fingerprint da host key e, com --sim, passa a confiar nela (fim do TOFU cego).
    Trust {
        alias: String,
        /// Confia sem novo prompt (use depois de conferir a fingerprint).
        #[arg(long)]
        sim: bool,
    },
    /// Roda um comando no host, com política e auditoria.
    /// Ex.: schematize vps exec srv-01 -- systemctl status app
    Exec {
        alias: String,
        /// Confirma um veredito `Confirm` (produção, encadeamento). NÃO atropela um `Deny`.
        #[arg(long)]
        confirmar: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        comando: Vec<String>,
    },
    /// Mostra o que já rodou (alias vazio = todos os hosts).
    Logs {
        #[arg(default_value = "")]
        alias: String,
        #[arg(long, default_value_t = 20)]
        n: usize,
        /// Mostra o transcript completo de cada linha.
        #[arg(long)]
        transcript: bool,
    },
    /// Ajusta a política de um host: modo e ambiente.
    Policy {
        alias: String,
        /// readonly | opsverbs | livre. Desconhecido vira `readonly` (falha fechada).
        #[arg(long)]
        modo: Option<String>,
        /// dev | hml | prd. Desconhecido vira `prd` (falha fechada).
        #[arg(long)]
        env: Option<String>,
    },
    /// Instala a chave PÚBLICA do perfil no authorized_keys do host (bootstrap de acesso).
    Authorize { alias: String },
    /// Pergunta ao host que nível de fronteira ele aguenta (somente leitura, nada é instalado).
    Probe { alias: String },
    /// Instala a MELHOR fronteira que o host aguentar (com sudo: shim do sistema; sem sudo:
    /// shim no home; host gerenciado: explica por que não dá e segue com a política do cliente).
    Bootstrap { alias: String },
    /// Catálogo de verbos do host — o vocabulário que o agente pode falar.
    Verbs {
        alias: String,
        /// Cria/atualiza um verbo (use junto com --cmd).
        #[arg(long)]
        add: Option<String>,
        /// O comando real que o verbo dispara no host.
        #[arg(long)]
        cmd: Option<String>,
        /// Remove um verbo.
        #[arg(long)]
        rm: Option<String>,
        /// Semeia um catálogo inicial plausível, sem sobrescrever o que já existe.
        #[arg(long)]
        seed: bool,
    },
    /// Remove um host do registro. A trilha de auditoria dele PERMANECE.
    Rm { alias: String },
    /// Liga/desliga o hook que barra SSH cru e leitura de chave privada no agente.
    Hooks {
        #[arg(long)]
        on: bool,
        #[arg(long)]
        off: bool,
    },
    /// (hook PreToolUse) veredito sobre uma tool use — lê o evento no stdin.
    #[command(hide = true)]
    Guard,
}

/// Servidor MCP — a porta CERTA do acesso remoto, com nome e schema que o agente entende.
#[derive(Subcommand)]
pub(crate) enum McpCmd {
    /// Roda o servidor (stdio, JSON-RPC). É o que o Claude Code invoca; raramente à mão.
    Serve,
    /// Registra o servidor no `.mcp.json` do projeto e libera as tools no settings.json.
    Install {
        /// Só mostra o que seria gravado, sem tocar em arquivo.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove o servidor do `.mcp.json` e as permissões do settings.json.
    Uninstall,
    /// Mostra o estado: servidor registrado? tools liberadas?
    Status,
}
