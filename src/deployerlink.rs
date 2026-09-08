//! PONTE com o **schematize deployer** — descoberta e chamada, nunca acoplamento.
//!
//! **O quê:** descobre se o Deployer está instalado, qual versão, e o invoca como
//! subprocesso quando o usuário pede.
//!
//! **Onde:** `schematize deployer <sub>` (CLI) e, adiante, o card da GUI.
//!
//! ## As três regras desta ponte
//!
//! **1. A ausência do Deployer nunca derruba o schematize** (piso 10). Todo caminho aqui
//! devolve "não instalado" como *estado*, não como erro. O schematize funcionava sem ele
//! ontem e continua funcionando hoje — a ponte é conveniência, não dependência.
//!
//! **2. Subprocesso, não biblioteca.** Ligar os dois crates faria o schematize compilar o
//! Deployer junto, e aí não haveria dois apps: haveria um monólito com dois nomes. É o que o
//! ADR-0010 decidiu evitar.
//!
//! **3. O Deployer nunca devolve segredo por aqui — e é o ponto.** Esta ponte lê **versão** e
//! **código de saída**. Se um dia alguém quiser trazer a passphrase do cofre ou o conteúdo de
//! uma chave por este canal, a resposta é não: o motivo de o Deployer existir é justamente
//! que o segredo **não** transite pelo processo onde o agente opera.

use crate::agentrun::resolve_bin;
use std::path::PathBuf;

/// Um app EXTERNO do ecossistema — instalável à parte, e que o schematize apenas conhece.
///
/// **Por que uma tabela e não um módulo por app:** a lógica de descobrir, versionar e
/// instalar é idêntica; duplicá-la por app seria a divergência esperando acontecer. O que
/// muda entre eles é só o nome do binário, a flag e a frase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppExterno {
    /// Nome do binário no `$PATH`.
    pub bin: &'static str,
    /// Flag do `install.sh` que o instala.
    pub flag: &'static str,
    /// Uma linha sobre o que ele faz — vai no `status`.
    pub sobre: &'static str,
}

/// Os apps externos que o schematize conhece.
pub const EXTERNOS: &[AppExterno] = &[
    AppExterno {
        bin: "schematize-deployer",
        flag: "--deployer",
        sobre: "SSH, VPS e acesso remoto auditado, com a credencial no cofre",
    },
    AppExterno {
        bin: "schematize-optimizer",
        flag: "--optimizer",
        sobre: "mede o ambiente de dev e põe cada software no seu teto de recurso",
    },
];

/// **O quê:** acha um app externo pelo nome do binário.
/// **Onde:** a CLI, ao despachar `schematize <app> …`.
pub fn externo(bin: &str) -> Option<&'static AppExterno> {
    EXTERNOS.iter().find(|a| a.bin == bin)
}

/// Nome do binário do Deployer.
pub const BIN: &str = "schematize-deployer";
/// Repositório, para a mensagem de instalação e para o `install.sh`.
pub const REPO: &str = "schematizeme/schematize_deployer_rs";
/// O `install.sh` que sabe instalar o Deployer — é o do SCHEMATIZE, com `--deployer`.
///
/// Repetido aqui em vez de reusar o do `selfupdate`: lá ele é privado e `#[cfg(unix)]`, e
/// esta mensagem tem de existir no Windows também (onde ela é justamente a única saída).
const INSTALL_SH: &str =
    "https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh";

/// O que se sabe do Deployer nesta máquina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Estado {
    /// Instalado, com o caminho e a versão que ele reporta.
    Instalado { caminho: PathBuf, versao: String },
    /// Não está no `$PATH` nem nos diretórios de fallback.
    Ausente,
    /// O binário existe mas não respondeu `--version` — instalação quebrada.
    ///
    /// É um terceiro estado de propósito: dizer "ausente" para um binário que ESTÁ lá mandaria
    /// a pessoa reinstalar o que já tem, e esconderia a causa real (permissão, biblioteca
    /// faltando, arquitetura errada).
    Quebrado { caminho: PathBuf, erro: String },
}

impl Estado {
    /// **O quê:** `true` só quando dá para usar o Deployer agora.
    /// **Onde:** a GUI e o `doctor`, para decidir entre "abrir" e "instalar".
    pub fn utilizavel(&self) -> bool {
        matches!(self, Estado::Instalado { .. })
    }
}

/// **O quê:** descobre o Deployer nesta máquina.
///
/// **Onde:** `schematize deployer status`, o `doctor` e a GUI.
///
/// Usa o mesmo `resolve_bin` do resto do app — que varre o `$PATH` **e** os diretórios de
/// fallback. Sem isso, o app aberto pelo lançador do desktop (que dá PATH mínimo) diria
/// "não instalado" sobre um Deployer que está em `~/.cargo/bin`.
pub fn descobrir_app(bin: &str) -> Estado {
    let Some(caminho) = resolve_bin(bin) else {
        return Estado::Ausente;
    };
    match crate::util::run(&caminho.to_string_lossy(), &["--version"]) {
        Ok(saida) => {
            let versao = saida.split_whitespace().nth(1).unwrap_or("?").to_string();
            Estado::Instalado { caminho, versao }
        }
        Err(e) => Estado::Quebrado { caminho, erro: e },
    }
}

/// **O quê:** o estado do Deployer. **Onde:** compat com quem já chamava.
pub fn descobrir() -> Estado {
    descobrir_app(BIN)
}

/// **O quê:** a linha de comando que instala o Deployer nesta máquina.
///
/// **Onde:** [`Estado::Ausente`] na CLI e na GUI. Função PURA — devolve o texto, não executa.
///
/// **Por que não instala sozinho:** instalar compila um app inteiro, pede rede e leva
/// minutos. Fazer isso como efeito colateral de um `status` seria surpresa cara. O comando
/// fica visível para a pessoa rodar quando quiser.
pub fn como_instalar_app(flag: &str) -> String {
    // O `install.sh` que tem a flag `--deployer` é o do SCHEMATIZE, não o do Deployer: é ele
    // que já sabe cuidar do Rust, das libs de build e do target compartilhado. O Deployer
    // entra como um quinto repo daquele mesmo fluxo.
    format!("curl -fsSL {INSTALL_SH} | bash -s -- {flag}")
}

/// **O quê:** como instalar o Deployer. **Onde:** compat.
pub fn como_instalar() -> String {
    como_instalar_app("--deployer")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `utilizavel` só é verdade no estado que de fato dá para usar. Sem esta distinção, um
    /// binário quebrado seria tratado como bom e a GUI abriria o que não abre.
    #[test]
    fn so_o_instalado_e_utilizavel() {
        let bom =
            Estado::Instalado { caminho: "/x/schematize-deployer".into(), versao: "0.2.1".into() };
        assert!(bom.utilizavel());
        assert!(!Estado::Ausente.utilizavel());
        let ruim =
            Estado::Quebrado { caminho: "/x/schematize-deployer".into(), erro: "libc".into() };
        assert!(!ruim.utilizavel(), "binário que não responde não pode contar como instalado");
    }

    /// A instrução de instalação cita o repo do schematize (é o `install.sh` dele que tem a
    /// flag) e a flag `--deployer`. Se um dia o caminho mudar, este teste é quem avisa.
    #[test]
    fn a_instrucao_de_instalar_e_acionavel() {
        let c = como_instalar();
        assert!(c.contains("--deployer"), "sem a flag o comando instala o app errado: {c}");
        assert!(c.contains("install.sh"), "{c}");
        assert!(c.starts_with("curl "), "tem de ser colável no terminal: {c}");
    }

    /// **A regra que a ponte existe para cumprir:** descobrir NUNCA falha. Numa máquina sem
    /// Deployer o resultado é `Ausente` — um estado —, não um erro que suba pro chamador.
    #[test]
    fn descobrir_nunca_falha_mesmo_sem_deployer() {
        // Não afirmamos QUAL estado (a máquina de quem roda pode ter o Deployer instalado);
        // afirmamos que a função retorna, sem panicar e sem `Result`.
        let e = descobrir();
        assert!(matches!(e, Estado::Instalado { .. } | Estado::Ausente | Estado::Quebrado { .. }));
    }
}
