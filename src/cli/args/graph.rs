//! O que SOBROU do `args/overdev.rs`: autostart e projetos.
//!
//! **Onde:** referenciado pelo `Cmd` em `args/mod.rs`.
//!
//! **Por que existe como arquivo próprio:** o `args/overdev.rs` juntava overdev, autostart,
//! projetos, caixa e grafo — cinco superfícies numa só, agrupadas por acidente de tamanho e
//! não por domínio. A E6 levou três; ficaram as duas que são do HUB: ligar um serviço no
//! login, e o registro de projetos que o seletor da janela principal lê.

use clap::Subcommand;

// O `Auto` (autostart do agente residente) também morava no `args/overdev.rs`, e também não
// era dele: ligar um serviço no login é do HUB, não do motor de um run. Ele fica aqui pela
// mesma razão que o `GraphCmd` — este arquivo é o que sobrou daquele agrupamento por acidente.

#[derive(Subcommand)]
pub(crate) enum Auto {
    /// Enable and start the agent (systemd --user + XDG autostart).
    Enable,
    /// Disable and remove the autostart.
    Disable,
}

// O `ProjectsCmd` FICA no hub, e é a exceção deliberada desta fase.
//
// `projects` lista e fixa os projetos de dev — o mesmo registro que o SELETOR da janela
// principal lê. Mandá-lo para o app do overdev faria o seletor do hub depender de um app
// opcional: sem ele instalado, a janela principal não saberia mais quais projetos existem.
//
// O app do overdev tem a própria cópia do módulo, porque precisa descobrir onde há run. Os
// dois leem a MESMA config, então respondem o mesmo — e quem manda no registro é o hub.

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
