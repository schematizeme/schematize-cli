//! O que os HOOKS do overdev fazem quando o `schematize-overdev` não está instalado.
//!
//! **O quê:** decide a resposta de `schematize overdev check` (Stop) e `schematize overdev
//! guard` (PreToolUse) sem o app, e detecta — só LENDO — se há um run ativo no cwd.
//!
//! **Onde:** [`crate::applink::interceptar`], no ramo "binário ausente" do delegado `overdev`.
//!
//! ## O bug que isto fecha
//!
//! O ramo de ausência saía `rc=1`. No Claude Code, hook com saída diferente de 0 ou 2 é erro
//! NÃO bloqueante: o run ativo deixava de ser vetado em silêncio. Aqui a saída é sempre `0`,
//! e o veto (quando cabe) viaja no JSON, que é o contrato do hook.
//!
//! ## Decisões
//!
//! - **`check` + run ativo:** bloqueia com o MESMO formato do app
//!   (`{"decision":"block","reason":…}`), dizendo que o app falta, que o run segue ativo e
//!   como resolver (instalar, ou `schematize overdev stop`).
//! - **`guard` NUNCA bloqueia sem o app.** O `guard` só veta `AskUserQuestion`, mas um veto
//!   cego aqui é pior que nenhum: o agente precisa de ferramentas livres para rodar a própria
//!   instalação, e um veto sem o motor que o sustenta travaria a sessão sem saída. Libera
//!   (rc 0, sem stdout) e AVISA no stderr. O veto de verdade do run é o `check`.
//! - **`stop_hook_active`:** o app NÃO o usa (limita laços com o contador `iterations` contra
//!   `max_iters`, gravado por ele). O hub não escreve estado — a ponte é read-only —, então
//!   não tem contador; a trava anti-laço é o `stop_hook_active` do Claude Code: se já estamos
//!   continuando por causa deste hook e o app segue ausente, o aviso foi dado e bloquear de
//!   novo prenderia o agente. Libera com aviso no stderr.
//! - **Sem run ativo:** rc 0, stdout vazio, dica curta no stderr. App ausente sem run não pode
//!   estragar a sessão de ninguém (piso 10).
//! - **Detecção = a do app, replicada:** `<cwd>/{.schematize,.overflow}/overdev/state.json`
//!   (o primeiro dir que existe; senão o legado `<cwd>/.overdev`), `mode == "active"`. É por
//!   cwd, sem subir diretórios — como o app. Arquivo ausente/ilegível = sem run. O MOTOR
//!   (contagem de itens, gate, orçamento) continua só no app (ADR-0012 F5).

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

/// Quanto do `stdin` do hook se lê: o JSON do Claude Code é pequeno; o teto evita travar o
/// hook num pipe que nunca fecha.
const LIMITE_STDIN: u64 = 1 << 20;

/// O comando que instala o app, o mesmo que o ramo de ausência do `applink` imprime.
const INSTALAR: &str = "schematize-market install schematize-overdev";

/// O que o hook deve fazer: o que escrever em stdout (se algo) e o aviso do stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resposta {
    /// JSON de bloqueio para o stdout; `None` = não escrever nada.
    pub stdout: Option<String>,
    /// Texto para o stderr (sempre há um: ausência de app nunca é muda).
    pub stderr: String,
}

/// **O quê:** o dir de estado do overdev em `raiz`, pela mesma regra do app: `.schematize`,
/// depois `.overflow`, depois o legado `.overdev`. PURA (só `is_dir`).
///
/// **Onde:** [`run_ativo_em`].
fn dir_do_overdev(raiz: &Path) -> PathBuf {
    for nome in [".schematize", ".overflow"] {
        let c = raiz.join(nome).join("overdev");
        if c.is_dir() {
            return c;
        }
    }
    let legado = raiz.join(".overdev");
    if legado.is_dir() {
        return legado;
    }
    raiz.join(".schematize").join("overdev")
}

/// **O quê:** há um run de overdev ATIVO em `raiz`? Só lê `state.json`; ausente, ilegível ou
/// com `mode` diferente de `"active"` = `false`.
///
/// **Onde:** [`responder`]. Não escreve nada, não conta item, não roda gate.
pub fn run_ativo_em(raiz: &Path) -> bool {
    let Ok(texto) = std::fs::read_to_string(dir_do_overdev(raiz).join("state.json")) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&texto)
        .ok()
        .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(|m| m == "active"))
        .unwrap_or(false)
}

/// **O quê:** o `stop_hook_active` do JSON que o Claude Code manda no `stdin`. PURA; texto
/// ruim ou campo ausente = `false`.
///
/// **Onde:** [`responder`].
pub fn stop_hook_ativo(stdin: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(stdin)
        .ok()
        .and_then(|v| v.get("stop_hook_active").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

/// **O quê:** a resposta do hook `sub` (`check` ou `guard`) sem o app. PURA — a regra, com os
/// fatos (run ativo? laço?) como parâmetros, é o que os testes cobrem.
///
/// **Onde:** [`executar`]. Devolve `None` para subcomando que não é hook (o chamador mantém o
/// erro de ausência de sempre).
pub fn responder(sub: &str, ativo: bool, stop_hook_active: bool) -> Option<Resposta> {
    if sub != "check" && sub != "guard" {
        return None;
    }
    if !ativo {
        return Some(Resposta {
            stdout: None,
            stderr: format!(
                "aviso: `schematize-overdev` não está instalado (sem run de overdev ativo aqui, \
                 nada a fazer). Instale com: {INSTALAR}"
            ),
        });
    }
    if sub == "guard" {
        return Some(Resposta {
            stdout: None,
            stderr: format!(
                "aviso: há um run de overdev ATIVO, mas `schematize-overdev` não está instalado \
                 — o veto está desligado. Instale com: {INSTALAR}"
            ),
        });
    }
    if stop_hook_active {
        return Some(Resposta {
            stdout: None,
            stderr: format!(
                "aviso: run de overdev ATIVO e `schematize-overdev` ainda ausente; não bloqueio \
                 de novo para não prender o agente em laço. Instale com: {INSTALAR}"
            ),
        });
    }
    let motivo = format!(
        "MODO OVERDEV — o app `schematize-overdev` NÃO está instalado, mas o run de overdev deste \
         projeto continua ATIVO, então não posso liberar a parada. Instale o app com: {INSTALAR} \
         (e retome o checklist) — ou, se quer encerrar o run, rode: schematize overdev stop \
         (esse comando também é do app; sem ele, troque \"mode\": \"active\" por \"stopped\" \
         em .schematize/overdev/state.json)"
    );
    Some(Resposta {
        stdout: Some(serde_json::json!({ "decision": "block", "reason": motivo }).to_string()),
        stderr: "aviso: `schematize-overdev` ausente com run ativo — bloqueei a parada.".into(),
    })
}

/// **O quê:** lê o `stdin` do hook (limitado; vazio se for terminal). Erro de leitura vira
/// texto vazio COM aviso no stderr — nunca é engolido em silêncio.
///
/// **Onde:** [`executar`], só para `check` (o `guard` não precisa do JSON).
fn ler_stdin() -> String {
    let mut s = std::io::stdin();
    if s.is_terminal() {
        return String::new();
    }
    let mut buf = String::new();
    if let Err(e) = s.by_ref().take(LIMITE_STDIN).read_to_string(&mut buf) {
        eprintln!("aviso: não consegui ler o stdin do hook: {e}");
    }
    buf
}

/// **O quê:** executa a resposta de ausência de `argv` (`["check"|"guard", …]`, já sem o
/// prefixo `overdev`). Devolve o rc (sempre `0`) ou `None` se não for hook.
///
/// **Onde:** [`crate::applink::interceptar`], quando `schematize-overdev` não resolve.
///
/// **Efeitos:** lê o cwd, o `state.json` e o stdin; escreve em stdout/stderr.
pub fn executar(args: &[String]) -> Option<i32> {
    let sub = args.first()?.as_str();
    if sub != "check" && sub != "guard" {
        return None;
    }
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aviso: cwd inacessível ({e}); assumo que não há run ativo");
            PathBuf::from("/nonexistent-schematize")
        }
    };
    let laco = sub == "check" && stop_hook_ativo(&ler_stdin());
    let r = responder(sub, run_ativo_em(&cwd), laco)?;
    if let Some(json) = r.stdout {
        println!("{json}");
    }
    eprintln!("{}", r.stderr);
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `guard` jamais bloqueia sem o app, com ou sem run: bloquear travaria a instalação.
    #[test]
    fn guard_nunca_bloqueia() {
        for ativo in [false, true] {
            let r = responder("guard", ativo, false).unwrap();
            assert!(r.stdout.is_none());
        }
    }

    /// Só `check` + ativo + sem laço produz JSON, e é o formato do app.
    #[test]
    fn check_bloqueia_so_com_run_ativo_e_sem_laco() {
        let b = responder("check", true, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&b.stdout.unwrap()).unwrap();
        assert_eq!(v["decision"], "block");
        assert!(responder("check", true, true).unwrap().stdout.is_none());
        assert!(responder("check", false, false).unwrap().stdout.is_none());
    }

    /// Subcomando que não é hook não é tratado aqui.
    #[test]
    fn so_check_e_guard() {
        assert!(responder("start", true, false).is_none());
        assert!(executar(&["start".to_string()]).is_none());
    }

    /// `stop_hook_active` só vale se for booleano `true`; lixo é `false`.
    #[test]
    fn le_stop_hook_active() {
        assert!(stop_hook_ativo(r#"{"stop_hook_active":true}"#));
        for ruim in ["", "{}", "null", r#"{"stop_hook_active":"true"}"#, "{ x"] {
            assert!(!stop_hook_ativo(ruim), "{ruim:?}");
        }
    }
}
