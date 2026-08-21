//! Registro dos hooks do overdev no settings.json do Claude Code.
//! O quê: liga/desliga o Stop hook (não parar) e o PreToolUse hook (vetar
//! AskUserQuestion) apontando pro próprio binário. Preserva o resto do settings.
//! Onde: chamado por `schematize overdev enable|disable`.

use crate::util::settings_path;
use std::path::{Path, PathBuf};
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
fn hook_cmd(_exe: &str, sub: &str) -> String {
    // NÃO embute o caminho de quem gerou o comando — nem o binário em execução.
    //
    // Embutir tornava o comando instável: gerado a partir de um `target/release/`, o
    // hook ficava pinado no diretório de build; gerado de um binário temporário, ficava
    // pinado nele pra sempre. E a checagem de "está atualizado?" trocava de resposta
    // conforme de onde o comando foi chamado, reescrevendo o settings à toa.
    //
    // A lista fixa cobre 100% das instalações que a casa produz: `~/.cargo/bin` (fonte
    // e updater) e `/usr/bin` (pacote .deb/.rpm), mais `/usr/local/bin` (self-update via
    // pkexec) e o PATH como rede. Sem binário nenhum, sai 0: hook que não bloqueia é
    // melhor que hook que quebra toda parada.
    //
    // `_exe` fica na assinatura porque os chamadores já a usam e ela documenta a
    // intenção ("registrar apontando pra este binário") — mas o comando é o mesmo em
    // qualquer máquina, e é isso que o torna previsível.
    //
    // `schematize` primeiro, `overflow` como rede. Houve um interregno curto em que o
    // app se chamou Overflow; quem gravou hook naquela janela ficou com o nome antigo
    // no settings.json, e a lista cobre os dois até a auto-cura (`refresh_hooks`)
    // regravar o comando canônico.
    format!(
        r#"for c in "$HOME/.cargo/bin/schematize" "$HOME/.local/bin/schematize" /usr/local/bin/schematize /usr/bin/schematize "$HOME/.cargo/bin/overflow" "$HOME/.local/bin/overflow" /usr/local/bin/overflow /usr/bin/overflow "$(command -v schematize 2>/dev/null)"; do [ -n "$c" ] && [ -x "$c" ] && exec "$c" {sub}; done; exit 0"#
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

// ---------------------------------------------------------------------------
// REPARO de hook quebrado — a classe inteira, não um caso.
//
// Três relatos, três formas diferentes do mesmo estrago:
//   `/usr/bin/schematize: not found`        (caminho do pacote .deb, removido)
//   `.claude/hooks/overdev-stop.sh: não existe` (caminho RELATIVO, do hook shell
//                                                legado, só válido numa pasta)
// O comando do hook mora no settings.json de quem LIGOU o overdev, e nada nunca
// revisitava aquilo. Em vez de tratar caso a caso: se o comando aponta pra um
// caminho que NÃO EXISTE, ele está quebrado — regrava com o resolvedor atual.
//
// O que NÃO é mexido: hook cujo caminho existe. Quem usa de propósito o fallback
// shell da skill (`~/.claude/skills/.../overdev-stop.sh`) continua com o dele.
// ---------------------------------------------------------------------------

/// Os caminhos de arquivo citados num comando de hook (tokens com `/`, `~` expandido).
fn caminhos_citados(cmd: &str) -> Vec<PathBuf> {
    cmd.split_whitespace()
        .map(|t| t.trim_matches(['"', '\'', ';']))
        .filter(|t| t.contains('/') && !t.starts_with('-'))
        .map(|t| match t.strip_prefix("~/") {
            Some(resto) => crate::util::home().join(resto),
            None => PathBuf::from(t),
        })
        .collect()
}

/// O comando do hook está QUEBRADO — cita caminhos e nenhum deles existe?
///
/// O nosso comando resiliente cita quatro caminhos de propósito (é o ponto dele:
/// tentar todos), então basta UM existir pra ele não ser considerado quebrado.
/// Comando sem caminho nenhum (ex.: só `schematize overdev check`, resolvido pelo
/// PATH) não é julgado aqui — não dá pra saber sem executar, e na dúvida não se mexe.
fn comando_quebrado(cmd: &str) -> bool {
    let caminhos = caminhos_citados(cmd);
    !caminhos.is_empty() && !caminhos.iter().any(|p| p.exists())
}

/// Repara os hooks do overdev num settings.json específico.
///
/// Regrava o comando quando ele está quebrado (aponta só pra caminho inexistente) OU
/// quando é o comando de uma versão anterior. Devolve `Ok(true)` se mexeu no arquivo.
/// Arquivo que não existe / não é JSON: não faz nada (não inventa settings pra ninguém).
pub fn repara_hooks_em(arquivo: &Path, exe: &str) -> Result<bool, String> {
    let Ok(texto) = fs::read_to_string(arquivo) else {
        return Ok(false);
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&texto) else {
        return Ok(false);
    };
    let mut mexeu = false;
    for (event, sub) in [("Stop", "overdev check"), ("PreToolUse", "overdev guard")] {
        let alvo = hook_cmd(exe, sub);
        let Some(arr) = root.get_mut("hooks").and_then(|h| h.get_mut(event)).and_then(|a| a.as_array_mut())
        else {
            continue;
        };
        for grupo in arr.iter_mut() {
            let Some(hs) = grupo.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
                continue;
            };
            for h in hs.iter_mut() {
                let Some(cmd) = h.get("command").and_then(|c| c.as_str()) else {
                    continue;
                };
                // Só mexemos em hook DO OVERDEV: ou cita o subcomando, ou é o script
                // legado da skill. Hook de terceiro no mesmo settings.json não é nosso.
                let nosso = cmd.contains(sub) || cmd.contains("overdev-stop.sh");
                if !nosso || cmd == alvo || !comando_quebrado(cmd) {
                    continue;
                }
                h["command"] = json!(alvo);
                mexeu = true;
            }
        }
    }
    if mexeu {
        let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        fs::write(arquivo, body).map_err(|e| e.to_string())?;
    }
    Ok(mexeu)
}

/// Os settings.json que valem pra uma sessão: o do usuário e, se houver projeto, os
/// dele (`.claude/settings.json` e `settings.local.json`). O hook quebrado do relato
/// estava no do PROJETO — olhar só o do usuário não bastava.
pub fn arquivos_de_settings(projeto: Option<&Path>) -> Vec<PathBuf> {
    let mut v = vec![settings_path()];
    if let Some(p) = projeto {
        v.push(p.join(".claude").join("settings.json"));
        v.push(p.join(".claude").join("settings.local.json"));
    }
    v
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

    /// Os TRÊS formatos de quebra já relatados em campo são reconhecidos, e o hook
    /// legítimo (caminho que existe) NÃO é tocado.
    #[test]
    fn reconhece_hook_quebrado_e_poupa_o_legitimo() {
        // 1) caminho do pacote .deb, removido pelo install do fonte (Mint)
        assert!(comando_quebrado("/usr/bin/schematize overdev check"));
        // 2) caminho RELATIVO do hook shell legado (a máquina do 3º relato):
        //    só resolveria se o Claude Code rodasse exatamente naquela pasta.
        assert!(comando_quebrado("bash .claude/hooks/overdev-stop.sh"));
        // 3) caminho absoluto de uma skill que não está instalada
        assert!(comando_quebrado("bash /nao/existe/overdev-stop.sh"));

        // LEGÍTIMO: um caminho que existe → não se mexe (quem usa o fallback shell
        // de propósito continua com o dele).
        let existente = format!("bash {}", std::env::current_exe().unwrap().display());
        assert!(!comando_quebrado(&existente));

        // Sem caminho nenhum (resolvido pelo PATH): não dá pra julgar sem executar,
        // e na dúvida não se mexe.
        assert!(!comando_quebrado("schematize overdev check"));
    }

    /// Repara o settings do PROJETO — era onde estava o hook relativo quebrado, e
    /// olhar só o do usuário não bastava.
    #[test]
    fn repara_settings_do_projeto() {
        let base = std::env::temp_dir().join(format!("hooks-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join(".claude")).unwrap();
        let arq = base.join(".claude").join("settings.json");
        std::fs::write(&arq, serde_json::to_string_pretty(&json!({
            "hooks": { "Stop": [ { "hooks": [
                { "type": "command", "command": "bash .claude/hooks/overdev-stop.sh" }
            ] } ] }
        })).unwrap()).unwrap();

        let mexeu = repara_hooks_em(&arq, "/home/x/.cargo/bin/schematize").unwrap();
        assert!(mexeu, "hook quebrado no projeto tinha de ser regravado");
        let novo: Value = serde_json::from_str(&std::fs::read_to_string(&arq).unwrap()).unwrap();
        let cmd = novo["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("command -v schematize"), "virou o resolvedor: {cmd}");
        assert!(cmd.ends_with("exit 0"), "e nunca derruba a parada: {cmd}");

        // Rodar de novo é no-op (não fica reescrevendo arquivo de ninguém à toa).
        assert!(!repara_hooks_em(&arq, "/home/x/.cargo/bin/schematize").unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Hook de TERCEIRO no mesmo settings.json não é nosso — não se toca.
    #[test]
    fn nao_mexe_em_hook_alheio() {
        let base = std::env::temp_dir().join(format!("hooks-alheio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let arq = base.join("settings.json");
        let alheio = "bash /caminho/que/nao/existe/formatador.sh";
        std::fs::write(&arq, serde_json::to_string_pretty(&json!({
            "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": alheio } ] } ] }
        })).unwrap()).unwrap();

        assert!(!repara_hooks_em(&arq, "/bin/schematize").unwrap(), "não é hook do overdev");
        let depois: Value = serde_json::from_str(&std::fs::read_to_string(&arq).unwrap()).unwrap();
        assert_eq!(depois["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap(), alheio);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// O comando gravado hoje sobrevive ao binário mudar de lugar: tenta o exe atual,
    /// depois ~/.cargo/bin, /usr/local/bin, /usr/bin e o PATH — e sai 0 se não achar
    /// nenhum (hook que não bloqueia é melhor que hook que quebra toda parada).
    #[test]
    fn comando_do_hook_tenta_todos_os_caminhos_e_nunca_falha() {
        let c = hook_cmd("/opt/schematize", "overdev check");
        for esperado in ["$HOME/.cargo/bin/schematize", "$HOME/.local/bin/schematize", "/usr/local/bin/schematize", "/usr/bin/schematize"] {
            assert!(c.contains(esperado), "faltou {esperado} em: {c}");
        }
        assert!(c.ends_with("exit 0"), "sem binário nenhum, o hook sai limpo: {c}");

        // O comando é IGUAL em qualquer máquina: não carrega o caminho de quem o gerou.
        // É isso que impede o hook de ficar pinado num diretório de build ou num binário
        // temporário, e que faz "está atualizado?" ter sempre a mesma resposta.
        assert!(!c.contains("/opt/schematize"), "não embute o caminho de quem gerou: {c}");
        assert_eq!(c, hook_cmd("/home/alguem/.cargo/bin/schematize", "overdev check"));
        assert_eq!(c, hook_cmd("/usr/bin/schematize", "overdev check"));
    }
}
