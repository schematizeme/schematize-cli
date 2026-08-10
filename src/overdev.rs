//! Motor do modo OVERDEV: dev contínuo até o checklist fechar, à prova de parada
//! prematura e SEM travar pra perguntar (parkeia a pergunta e segue).
//! O quê: start/check/guard/status/hold/ask/stop. check = Stop hook; guard = PreToolUse
//! hook que VETA AskUserQuestion enquanto em overdev. Onde: chamado por main.

use crate::settings;
use crate::util::{self, self_exe};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const DEFAULT_MAX_ITERS: u64 = 200;
const QUESTIONS_FILE: &str = "PERGUNTAS-OVERDEV.txt";

#[derive(Serialize, Deserialize)]
struct OverState {
    mode: String, // active | done | blocked | stopped
    max_iters: u64,
    objetivo: String,
    started: u64,
}

fn dir() -> PathBuf {
    PathBuf::from(".overdev")
}
fn state_file() -> PathBuf {
    dir().join("state.json")
}
fn checklist() -> PathBuf {
    dir().join("CHECKLIST.md")
}
fn iters_file() -> PathBuf {
    dir().join("iterations")
}

fn load() -> Option<OverState> {
    fs::read_to_string(state_file()).ok().and_then(|s| serde_json::from_str(&s).ok())
}
fn save(st: &OverState) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    fs::write(state_file(), serde_json::to_string_pretty(st).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// (abertos, feitos, on-hold) do checklist.
fn counts() -> (usize, usize, usize) {
    let s = fs::read_to_string(checklist()).unwrap_or_default();
    let (mut open, mut done, mut hold) = (0, 0, 0);
    for l in s.lines() {
        let t = l.trim_start();
        if t.starts_with("- [ ]") {
            open += 1;
        } else if t.starts_with("- [x]") || t.starts_with("- [X]") {
            done += 1;
        } else if t.starts_with("- [~]") {
            hold += 1;
        }
    }
    (open, done, hold)
}

fn open_items() -> Vec<String> {
    let s = fs::read_to_string(checklist()).unwrap_or_default();
    s.lines()
        .filter(|l| l.trim_start().starts_with("- [ ]"))
        .take(8)
        .map(|l| l.trim().replace('"', "'"))
        .collect()
}

/// Roda o gate opcional (.overdev/gate.sh). true se não há gate ou se passa.
fn gate_ok() -> bool {
    let g = dir().join("gate.sh");
    if !g.exists() {
        return true;
    }
    util::run("bash", &[g.to_str().unwrap()]).is_ok()
}

fn drain_stdin() {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
}

fn print_block(reason: &str) {
    let v = serde_json::json!({ "decision": "block", "reason": reason });
    println!("{v}");
}

/// Inicia um run de overdev no diretório atual.
pub fn start(objetivo: &str, max_iters: Option<u64>) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let st = OverState {
        mode: "active".into(),
        max_iters: max_iters.unwrap_or(DEFAULT_MAX_ITERS),
        objetivo: objetivo.to_string(),
        started: util::now_unix(),
    };
    save(&st)?;
    fs::write(iters_file(), "0").map_err(|e| e.to_string())?;
    if !checklist().exists() {
        let tpl = format!(
            "# OVERDEV — checklist\n\nObjetivo: {objetivo}\n\n> Um item por linha, verificável. `- [ ]` aberto · `- [x]` feito · `- [~]` on-hold (pergunta parkeada).\n\n- [ ] (gere o checklist exaustivo aqui)\n"
        );
        fs::write(checklist(), tpl).map_err(|e| e.to_string())?;
    }
    // Garante gitignore do control-plane.
    let gi = Path::new(".gitignore");
    let cur = fs::read_to_string(gi).unwrap_or_default();
    if !cur.contains(".overdev") {
        let _ = fs::write(gi, format!("{cur}\n.overdev/\n"));
    }
    println!("overdev ATIVO. Objetivo: {objetivo}");
    println!("Preencha .overdev/CHECKLIST.md (exaustivo). O agente não pode parar até fechar.");
    Ok(())
}

/// Stop hook: rejeita a parada enquanto houver item aberto (on-hold não conta).
pub fn check() {
    drain_stdin();
    let st = match load() {
        Some(s) if s.mode == "active" => s,
        _ => return, // inerte
    };
    // budget
    let mut it: u64 = fs::read_to_string(iters_file()).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    it += 1;
    let _ = fs::write(iters_file(), it.to_string());
    if it > st.max_iters {
        return; // guardrail anti-loop: permite parar
    }
    let (open, _done, hold) = counts();
    if open == 0 && gate_ok() {
        return; // tudo feito ou on-hold + gate verde → parada legítima
    }
    let reason = if open == 0 && !gate_ok() {
        format!("MODO OVERDEV — NÃO PARE. O checklist está todo marcado mas o gate (.overdev/gate.sh) FALHOU (ciclo {it}/{}). Conserte o que o gate acusa e rode de novo. Só pare com o gate verde.", st.max_iters)
    } else {
        let items = open_items().join("\\n");
        format!(
            "MODO OVERDEV — NÃO PARE E NÃO DIGA QUE TERMINOU. Faltam {open} item(ns) abertos ({hold} on-hold, ciclo {it}/{}). NÃO fale com o usuário ainda. Pegue o PRÓXIMO item aberto, implemente, VERIFIQUE (gate/teste), marque '- [x]'. Se travar num item por dúvida: NÃO use AskUserQuestion — escreva a pergunta em ./{QUESTIONS_FILE}, marque o item como '- [~]' (on-hold) e SIGA para os outros. Itens abertos:\\n{items}\\nSó é permitido parar quando não sobrar '- [ ]' (tudo '- [x]' ou '- [~]') e o gate passar.",
            st.max_iters
        )
    };
    print_block(&reason);
}

/// PreToolUse hook (matcher AskUserQuestion): veta a pergunta bloqueante em overdev.
pub fn guard() {
    drain_stdin();
    let active = matches!(load(), Some(s) if s.mode == "active");
    if !active {
        return; // fora de overdev: libera normalmente
    }
    let reason = format!(
        "VETADO em OVERDEV: nada de parar pra perguntar com pool bloqueante. Escreva a pergunta em ./{QUESTIONS_FILE} (na base do projeto), marque o item correspondente como '- [~]' (on-hold) no .overdev/CHECKLIST.md com `schematize overdev hold`, e CONTINUE os outros itens. As perguntas serão respondidas quando o usuário voltar."
    );
    let v = serde_json::json!({
        "decision": "block",
        "reason": reason,
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        }
    });
    println!("{v}");
}

/// Parkeia uma pergunta: registra no txt da base e marca o item como on-hold.
pub fn park(item_substr: &str, pergunta: &str) -> Result<(), String> {
    // 1) registra a pergunta na base do projeto.
    let mut q = fs::read_to_string(QUESTIONS_FILE).unwrap_or_default();
    q.push_str(&format!("[{}] item: {item_substr}\n  pergunta: {pergunta}\n\n", util::now_unix()));
    fs::write(QUESTIONS_FILE, q).map_err(|e| e.to_string())?;
    // 2) marca o primeiro item aberto que casa como on-hold.
    hold(item_substr)?;
    println!("pergunta parkeada em ./{QUESTIONS_FILE}; item marcado on-hold. Siga os demais.");
    Ok(())
}

/// Marca o primeiro `- [ ]` que contém `substr` como `- [~]` (on-hold).
pub fn hold(substr: &str) -> Result<(), String> {
    let s = fs::read_to_string(checklist()).map_err(|e| e.to_string())?;
    let mut done = false;
    let out: Vec<String> = s
        .lines()
        .map(|l| {
            if !done && l.trim_start().starts_with("- [ ]") && l.contains(substr) {
                done = true;
                l.replacen("- [ ]", "- [~]", 1)
            } else {
                l.to_string()
            }
        })
        .collect();
    fs::write(checklist(), out.join("\n")).map_err(|e| e.to_string())?;
    if !done {
        return Err(format!("nenhum item aberto contém '{substr}'"));
    }
    Ok(())
}

/// Mostra o estado atual.
pub fn status() {
    match load() {
        None => println!("sem overdev ativo neste diretório."),
        Some(st) => {
            let (open, done, hold) = counts();
            let it = fs::read_to_string(iters_file()).unwrap_or_else(|_| "0".into());
            println!("modo={} objetivo={}", st.mode, st.objetivo);
            println!("checklist: {done} feitos · {open} abertos · {hold} on-hold");
            println!("ciclos={} / max={}", it.trim(), st.max_iters);
            if let Ok(q) = fs::read_to_string(QUESTIONS_FILE) {
                let n = q.lines().filter(|l| l.starts_with('[')).count();
                if n > 0 {
                    println!("perguntas parkeadas: {n} (ver ./{QUESTIONS_FILE})");
                }
            }
        }
    }
}

/// Encerra o run (mode=stopped) — o hook volta a ser inerte.
pub fn stop() -> Result<(), String> {
    if let Some(mut st) = load() {
        st.mode = "stopped".into();
        save(&st)?;
    }
    println!("overdev encerrado.");
    Ok(())
}

/// Liga os hooks no settings.json (Stop + PreToolUse), apontando pro binário atual.
pub fn enable() -> Result<(), String> {
    settings::enable(&self_exe())?;
    println!("overdev habilitado (hooks Stop + PreToolUse registrados). Inerte até `schematize overdev start`.");
    Ok(())
}

/// Desliga os hooks do overdev do settings.json.
pub fn disable() -> Result<(), String> {
    settings::disable()?;
    println!("overdev desabilitado (hooks removidos do settings.json).");
    Ok(())
}
