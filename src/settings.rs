//! Registro dos hooks do overdev no settings.json do Claude Code.
//! O quê: liga/desliga o Stop hook (não parar) e o PreToolUse hook (vetar
//! AskUserQuestion) apontando pro próprio binário. Preserva o resto do settings.
//! Onde: chamado por `schematize overdev enable|disable`.

use crate::util::settings_path;
use serde_json::{json, Value};
use std::fs;

/// Um grupo de hooks contém um comando com este trecho?
fn group_has(group: &Value, needle: &str) -> bool {
    group.get("hooks").and_then(|h| h.as_array()).is_some_and(|arr| {
        arr.iter().any(|h| {
            h.get("command").and_then(|c| c.as_str()).is_some_and(|c| c.contains(needle))
        })
    })
}

/// Lê o settings.json como objeto (ou objeto vazio se não existir/inválido).
fn load() -> Value {
    match fs::read_to_string(settings_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    }
}

fn save(v: &Value) -> Result<(), String> {
    let p = settings_path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    fs::write(&p, body).map_err(|e| e.to_string())
}

/// Garante que `hooks[event]` (array) tenha o grupo `group` se ainda não houver `needle`.
fn ensure_group(root: &mut Value, event: &str, needle: &str, group: Value) {
    let hooks = root.as_object_mut().unwrap().entry("hooks").or_insert_with(|| json!({}));
    let arr = hooks.as_object_mut().unwrap().entry(event).or_insert_with(|| json!([]));
    let arr = arr.as_array_mut().unwrap();
    // REMOVE qualquer versão anterior do hook (mesmo needle) antes de adicionar — assim `enable`
    // ATUALIZA o comando em vez de só adicionar-se-ausente. Sem isto, um hook antigo com caminho
    // absoluto stale (ex.: `/usr/bin/schematize` de um pacote removido) nunca era trocado pelo
    // resolvedor resiliente ao re-rodar `overdev enable`.
    arr.retain(|g| !group_has(g, needle));
    arr.push(group);
}

/// Os hooks do overdev estão registrados no settings.json?
pub fn overdev_enabled() -> bool {
    load()
        .get("hooks")
        .and_then(|h| h.get("Stop"))
        .and_then(|a| a.as_array())
        .is_some_and(|arr| arr.iter().any(|g| group_has(g, "overdev check")))
}

/// O settings.json existe e é JSON válido? (None = não existe; Some(false) = inválido.)
pub fn settings_valid() -> Option<bool> {
    match fs::read_to_string(settings_path()) {
        Ok(s) => Some(serde_json::from_str::<Value>(&s).is_ok()),
        Err(_) => None,
    }
}

/// Comando de hook RESILIENTE: resolve o `schematize` em RUNTIME (o `exe` de agora como 1ª pista,
/// depois ~/.cargo/bin, /usr/local/bin, /usr/bin e o PATH) e roda `<sub>`. Se NÃO achar o binário,
/// sai 0 (não bloqueia, não erra) — antes gravava um caminho absoluto que virava stale (ex.: o
/// pacote saía e ficava `/usr/bin/schematize: not found` em todo Stop). "Prever macacos".
fn hook_cmd(exe: &str, sub: &str) -> String {
    format!(
        r#"for c in "{exe}" "$HOME/.cargo/bin/schematize" /usr/local/bin/schematize /usr/bin/schematize "$(command -v schematize 2>/dev/null)"; do [ -n "$c" ] && [ -x "$c" ] && exec "$c" {sub}; done; exit 0"#
    )
}

/// Liga os dois hooks do overdev. O comando resolve o binário em runtime (resiliente a mudança de
/// caminho / pacote removido), com `exe` (o binário atual) como primeira pista.
pub fn enable(exe: &str) -> Result<(), String> {
    let mut root = load();
    if !root.is_object() {
        root = json!({});
    }
    let stop_cmd = hook_cmd(exe, "overdev check");
    let guard_cmd = hook_cmd(exe, "overdev guard");
    ensure_group(&mut root, "Stop", "overdev check",
        json!({ "hooks": [ { "type": "command", "command": stop_cmd } ] }));
    ensure_group(&mut root, "PreToolUse", "overdev guard",
        json!({ "matcher": "AskUserQuestion", "hooks": [ { "type": "command", "command": guard_cmd } ] }));
    save(&root)
}

/// Os hooks registrados batem com o comando que ESTA versão gravaria?
///
/// `false` quando o `settings.json` guarda um comando de uma versão anterior — o caso
/// real: um hook com caminho absoluto (`/usr/bin/schematize`, do pacote .deb) que o
/// install do fonte removeu, deixando `not found` em todo Stop. O `enable` já grava o
/// resolvedor resiliente, mas ninguém re-roda `enable` ao atualizar o app: o comando
/// velho fica lá pra sempre.
pub fn hooks_atualizados(exe: &str) -> bool {
    let root = load();
    let esperado = [("Stop", "overdev check"), ("PreToolUse", "overdev guard")];
    esperado.iter().all(|(event, sub)| {
        let alvo = hook_cmd(exe, sub);
        root.get("hooks")
            .and_then(|h| h.get(event))
            .and_then(|a| a.as_array())
            .is_some_and(|arr| {
                arr.iter().any(|g| {
                    g.get("hooks").and_then(|h| h.as_array()).is_some_and(|hs| {
                        hs.iter().any(|h| h.get("command").and_then(|c| c.as_str()) == Some(alvo.as_str()))
                    })
                })
            })
    })
}

/// Regrava os hooks com o comando desta versão — SÓ se o overdev já estiver ligado.
///
/// É a auto-cura: quem ligou o overdev numa versão antiga carrega o comando daquela
/// versão no `settings.json`, e atualizar o app não mexia nisso. Chamado de onde roda
/// com frequência (o agente) e do `doctor`. Devolve `true` se regravou.
/// Não liga hook em quem não pediu: overdev desligado → não faz nada.
pub fn refresh_hooks(exe: &str) -> Result<bool, String> {
    if !overdev_enabled() || hooks_atualizados(exe) {
        return Ok(false);
    }
    enable(exe)?;
    Ok(true)
}

/// Remove os grupos de hook do overdev (Stop/PreToolUse) sem tocar no resto.
pub fn disable() -> Result<(), String> {
    let mut root = load();
    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (event, needle) in [("Stop", "overdev check"), ("PreToolUse", "overdev guard")] {
            if let Some(arr) = hooks.get_mut(event).and_then(|a| a.as_array_mut()) {
                arr.retain(|g| !group_has(g, needle));
            }
        }
    }
    save(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REGRESSÃO: hook gravado por uma versão ANTIGA continua no settings.json depois
    /// do update. Foi o caso real de uma máquina já na versão mais nova recebendo
    /// `/usr/bin/schematize: not found` em todo Stop: o pacote .deb que instalou aquele
    /// caminho tinha sido removido pelo install do fonte, mas o comando ficou.
    #[test]
    fn hook_de_versao_antiga_e_detectado_como_desatualizado() {
        // O que a v-antiga gravava: caminho absoluto, cru, sem resolvedor.
        let antigo = json!({
            "hooks": {
                "Stop": [ { "hooks": [ { "type": "command", "command": "/usr/bin/schematize overdev check" } ] } ],
                "PreToolUse": [ { "matcher": "AskUserQuestion",
                                  "hooks": [ { "type": "command", "command": "/usr/bin/schematize overdev guard" } ] } ]
            }
        });
        // Continua "ligado" (é por isso que ninguém percebia): o needle casa.
        assert!(
            antigo.get("hooks").and_then(|h| h.get("Stop")).and_then(|a| a.as_array())
                .is_some_and(|arr| arr.iter().any(|g| group_has(g, "overdev check"))),
            "o hook velho ainda casa como 'ligado' — por isso o enable pulava"
        );
        // Mas NÃO é o comando desta versão: é isso que passa a ser detectado.
        let alvo = hook_cmd("/home/x/.cargo/bin/schematize", "overdev check");
        assert_ne!(alvo, "/usr/bin/schematize overdev check");
        assert!(alvo.contains("command -v schematize"), "o comando atual resolve em runtime");
    }

    /// O comando gravado hoje sobrevive ao binário mudar de lugar: tenta o exe atual,
    /// depois ~/.cargo/bin, /usr/local/bin, /usr/bin e o PATH — e sai 0 se não achar
    /// nenhum (hook que não bloqueia é melhor que hook que quebra toda parada).
    #[test]
    fn comando_do_hook_tenta_todos_os_caminhos_e_nunca_falha() {
        let c = hook_cmd("/opt/schematize", "overdev check");
        for esperado in ["/opt/schematize", "$HOME/.cargo/bin/schematize", "/usr/local/bin/schematize", "/usr/bin/schematize"] {
            assert!(c.contains(esperado), "faltou {esperado} em: {c}");
        }
        assert!(c.ends_with("exit 0"), "sem binário nenhum, o hook sai limpo: {c}");
    }
}
