//! PONTE com o **schematize-overdev** — descoberta e leitura, nunca acoplamento.
//!
//! **O quê:** pergunta ao app do overdev o que ele sabe: o estado dos hooks e o progresso de
//! um run.
//!
//! **Onde:** o `doctor`, o `status` e o relatório de diagnóstico.
//!
//! ## Este é o app cuja ausência mais dói, e ainda assim a regra é a mesma
//!
//! Sem ele, o `doctor` não sabe dizer se os hooks estão ligados e o `status` não mostra o run.
//! **Isso é um estado, não um erro** (piso 10): o hub continua listando, instalando,
//! diagnosticando o resto e delegando para os outros apps.
//!
//! **Os HOOKS continuam funcionando sem esta ponte**, e é o ponto: eles não passam por aqui.
//! O Claude Code executa o comando gravado no `settings.json`, que procura o
//! `schematize-overdev` e, se não achar, cai no `schematize overdev …` — que o `applink`
//! encaminha. Esta ponte é para o hub PERGUNTAR; o caminho crítico é outro.

/// Nome do binário do overdev no `$PATH`.
pub const BIN: &str = "schematize-overdev";

/// O que o app diz sobre os hooks do Claude Code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hooks {
    /// `"valido"`, `"invalido"` ou `"ausente"` — três estados, e não dois.
    pub settings: String,
    pub ligados: bool,
    /// O comando gravado é o desta versão? `false` pede `hooks --repair`.
    pub em_dia: bool,
}

/// **O quê:** roda um subcomando do app e devolve o stdout. `None` se ele não responder.
fn perguntar(args: &[&str]) -> Option<String> {
    let bin = crate::binlink::resolve_bin(BIN)?;
    let o = std::process::Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// **O quê:** o estado dos hooks. `None` sem o app instalado.
///
/// **Onde:** o `doctor` e o `status`.
pub fn hooks() -> Option<Hooks> {
    ler_hooks(&perguntar(&["hooks", "--json"])?)
}

/// **O quê:** o estado dos hooks a partir do TEXTO. PURA, e é onde os testes entram.
pub fn ler_hooks(texto: &str) -> Option<Hooks> {
    let v: serde_json::Value = serde_json::from_str(texto).ok()?;
    Some(Hooks {
        settings: v.get("settings")?.as_str()?.to_string(),
        ligados: v.get("ligados").and_then(|b| b.as_bool()) == Some(true),
        em_dia: v.get("em_dia").and_then(|b| b.as_bool()) == Some(true),
    })
}

/// **O quê:** manda o app CONSERTAR os hooks. `Err` com o motivo quando não dá.
///
/// **Onde:** o `doctor --fix`.
///
/// **Aqui o erro é ERRO.** A leitura degrada em silêncio porque um diagnóstico sem a linha dos
/// hooks ainda é um diagnóstico; um conserto que não aconteceu tem de aparecer, senão a pessoa
/// sai achando que arrumou.
pub fn reparar_hooks() -> Result<(), String> {
    let bin = crate::binlink::resolve_bin(BIN)
        .ok_or_else(|| format!("`{BIN}` não está instalado — é ele que registra os hooks"))?;
    let o = std::process::Command::new(bin)
        .args(["hooks", "--repair"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("não consegui executar `{BIN}`: {e}"))?;
    if o.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&o.stderr);
    let err = err.trim();
    Err(if err.is_empty() { format!("`{BIN} hooks --repair` falhou") } else { err.to_string() })
}

/// **O quê:** a linha curta do run do diretório atual. `None` sem o app ou sem run.
///
/// **Onde:** o `schematize status`.
pub fn progresso_breve() -> Option<String> {
    let t = perguntar(&["progresso"])?;
    let l = t.trim();
    (!l.is_empty()).then(|| l.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_o_estado_dos_hooks() {
        let h = ler_hooks(r#"{"settings":"valido","ligados":true,"em_dia":false}"#).expect("lê");
        assert_eq!(h.settings, "valido");
        assert!(h.ligados);
        assert!(!h.em_dia);
    }

    /// **`ausente` é um terceiro estado, e o `doctor` o mostra assim.** Um `settings.json` que
    /// não existe é o caso de quem nunca ligou os hooks; chamar isso de "inválido" mandaria a
    /// pessoa consertar um arquivo que não tem nada de errado — ele só não existe ainda.
    #[test]
    fn ausente_nao_e_invalido() {
        let h = ler_hooks(r#"{"settings":"ausente","ligados":false,"em_dia":false}"#).expect("lê");
        assert_eq!(h.settings, "ausente");
        assert!(!h.ligados);
    }

    /// **Booleano duvidoso conta como FALSE.** Dizer "em dia" sobre um documento que não
    /// afirmou isso faria o `doctor` deixar de oferecer o conserto — e o hook velho seguiria
    /// rodando sem ninguém saber.
    #[test]
    fn booleano_duvidoso_e_falso() {
        let h = ler_hooks(r#"{"settings":"valido","ligados":"sim","em_dia":1}"#).expect("lê");
        assert!(!h.ligados);
        assert!(!h.em_dia);
    }

    /// Documento ruim vira `None`: sem o app, o `doctor` diz que não sabe — não que está
    /// tudo bem.
    #[test]
    fn documento_ruim_vira_none() {
        let fundo = format!("{}{}", "[".repeat(3000), "]".repeat(3000));
        for lixo in ["", "null", "[]", "0", "{ nao e json", "\u{0}", &fundo, "{}"] {
            assert!(ler_hooks(lixo).is_none(), "{lixo:?}");
        }
    }
}
