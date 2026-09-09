//! O `schematize env` encaminha ao `schematize-market`.
//!
//! **O quê:** traduz cada subcomando de `env` no comando equivalente do gestor e o executa,
//! herdando o terminal.
//!
//! **Onde:** o despacho de `Cmd::Env` no `main`.
//!
//! ## Por que encaminhar em vez de ter o código
//!
//! O ADR-0012 disse que o market seria o dono de instalar programas, mas a extração **copiou**
//! `environments/` em vez de mover: ficaram duas cópias de 2.391 linhas, e elas **já
//! divergiram**. O custo apareceu em uso — o `install csharp --method distro` ganhou, no
//! market, a capacidade de adicionar o repo do fornecedor; o `schematize env` do hub seguiu
//! recusando, porque é outra cópia. Corrigir nos dois lugares seria pagar o preço da
//! duplicação de novo, e mais uma vez no próximo conserto.
//!
//! ## Por que o comando NÃO é removido
//!
//! Superfície de CLI é contrato, e há snapshot que a trava. Quem escreveu
//! `schematize env install go` num script não pode descobrir a mudança pelo erro. O
//! encaminhador custa dez linhas e não duplica nada: o código passa a existir só no market.
//!
//! ## Por que o caminho é resolvido, e não o nome puro
//!
//! Já custou um bug reportado em uso: a janela aberta pelo lançador do desktop tem PATH mínimo
//! (sem `~/.cargo/bin`), e o terminal que ela abre herda. Nome puro morre ali com
//! `command not found`, sobre um gestor instalado.

use crate::agentrun::resolve_bin;
use crate::deployerlink::GESTOR;

/// **O quê:** roda `schematize-market <args>` herdando o terminal.
///
/// **Onde:** todos os subcomandos de [`crate::cli::args::EnvCmd`].
///
/// **Herda o terminal de propósito:** instalar linguagem pede sudo, compila e fala o tempo
/// todo. Capturar a saída deixaria a pessoa olhando um cursor parado, e o pedido de senha não
/// teria onde aparecer.
pub fn encaminhar(args: &[String]) -> Result<(), String> {
    let Some(gestor) = resolve_bin(GESTOR) else {
        return Err(format!(
            "o gestor `{GESTOR}` não está instalado — é ele que cuida de instalar linguagens \
             e ferramentas.\n  Instale-o com:\n    curl -fsSL {} | bash",
            crate::upgrade::INSTALL_SH
        ));
    };
    let st = std::process::Command::new(&gestor)
        .args(args)
        .status()
        .map_err(|e| format!("não consegui executar `{}`: {e}", gestor.display()))?;
    if st.success() {
        return Ok(());
    }
    // O gestor já explicou o que houve na saída dele; repetir aqui daria duas mensagens para
    // um erro. O código de saída é propagado como erro para o `main` sair diferente de zero.
    Err(format!(
        "`{} {}` terminou com erro ({}).",
        GESTOR,
        args.join(" "),
        st.code().map(|c| c.to_string()).unwrap_or_else(|| "sinal".into())
    ))
}

/// **O quê:** monta os argumentos do gestor a partir de um subcomando de `env`. PURA.
///
/// **Onde:** [`encaminhar`], via o despacho do `main`.
///
/// Separada da execução porque é ela que carrega a tradução — e tradução sem teste é onde uma
/// flag se perde no caminho sem ninguém notar até alguém precisar dela.
pub fn args_de_env(
    sub: &str,
    alvo: &str,
    method: Option<&str>,
    to: Option<&str>,
    dry_run: bool,
    yes: bool,
) -> Vec<String> {
    let mut v = vec![sub.to_string()];
    if !alvo.is_empty() {
        v.push(alvo.to_string());
    }
    if let Some(t) = to {
        v.push("--to".into());
        v.push(t.to_string());
    }
    if let Some(m) = method {
        v.push("--method".into());
        v.push(m.to_string());
    }
    if dry_run {
        v.push("--dry-run".into());
    }
    if yes {
        v.push("--yes".into());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tradução preserva o alvo e as flags — perder um `--method` faria o gestor escolher
    /// sozinho um caminho de instalação que a pessoa não pediu.
    #[test]
    fn install_leva_metodo_e_flags() {
        let a = args_de_env("install", "csharp", Some("distro"), None, false, true);
        assert_eq!(a, ["install", "csharp", "--method", "distro", "--yes"]);
    }

    /// `switch` usa `--to`, e ele não pode virar posicional no caminho.
    #[test]
    fn switch_leva_o_destino_em_to() {
        let a = args_de_env("switch", "rust", None, Some("official"), true, false);
        assert_eq!(a, ["switch", "rust", "--to", "official", "--dry-run"]);
    }

    /// `list` não tem alvo; um argumento vazio viraria um alvo `""` que o gestor não conhece.
    #[test]
    fn list_nao_inventa_alvo() {
        assert_eq!(args_de_env("list", "", None, None, false, false), ["list"]);
    }

    /// Sem `--method`, a flag não aparece — passá-la vazia daria `--method` sem valor e o
    /// `clap` do gestor reclamaria de sintaxe em vez de instalar.
    #[test]
    fn sem_metodo_a_flag_nao_entra() {
        let a = args_de_env("remove", "go", None, None, false, false);
        assert_eq!(a, ["remove", "go"]);
    }
}
