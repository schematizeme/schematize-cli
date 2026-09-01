//! Archive de EVOLUÇÃO do projeto — o repo git privado (`<projeto>/<projeto>_archive/`) que
//! DOCUMENTA a história. O quê: `sync` materializa a estrutura obrigatória, extrai o CHAT da sessão
//! (do transcript do claude → `chats/`), gera o `context_agent/#N.txt` (contexto PORTÁVEL entre PCs/
//! apps) e commita no repo do archive. Onde: chamado por main (`schematize archive ...`) e pela GUI.
//!
//! Criticidade 0: guardar o histórico não é opcional (ver skill schematize-archive + engineering §28).

use crate::overdev;
use crate::util;
use std::fs;
use std::path::{Path, PathBuf};

/// Subdirs canônicos do archive (a estrutura obrigatória).
const SUBDIRS: &[&str] =
    &["overdev", "chats", "decisoes", "index", "audit", "pentest", "context_agent"];

/// Materializa a estrutura + extrai o chat da sessão pro `chats/` + gera o `context_agent/#N.txt` +
/// commita no repo do archive. Devolve um resumo do que foi feito.
pub fn sync(root: &Path) -> Result<String, String> {
    let arch = overdev::archive_dir(root).ok_or("não consegui derivar o dir do archive")?;
    for d in SUBDIRS {
        let _ = fs::create_dir_all(arch.join(d));
    }
    ensure_git_repo(&arch);
    let chats = sync_chats(root, &arch);
    let ctx = write_context_agent(root, &arch)?;
    let msg = format!("archive sync: {chats} · {ctx}");
    commit_archive(&arch, &msg);
    Ok(format!("archive em {}\n  {chats}\n  {ctx}", arch.display()))
}

/// Garante que o archive é um repo git (init + README se preciso). Best-effort.
fn ensure_git_repo(arch: &Path) {
    if !arch.join(".git").is_dir() {
        let _ =
            std::process::Command::new("git").arg("-C").arg(arch).arg("init").arg("-q").status();
    }
    let readme = arch.join("README.md");
    if !readme.exists() {
        let _ = fs::write(
            &readme,
            "# archive — evolução documentada do projeto\n\nRepositório PRIVADO obrigatório (criticidade 0), irmão dos microserviços.\n`overdev/` `chats/` `decisoes/` `index/` `audit/` `pentest/` `context_agent/`.\n",
        );
    }
}

/// Diretório de transcript do claude pra este projeto: `~/.claude/projects/<abs-path com / → ->`.
fn transcript_dir(root: &Path) -> Option<PathBuf> {
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let encoded = canon.to_string_lossy().replace(['/', '.'], "-");
    let d = util::home().join(".claude").join("projects").join(encoded);
    if d.is_dir() {
        Some(d)
    } else {
        None
    }
}

/// Extrai cada mensagem (você + respostas) dos transcripts `.jsonl` do projeto → `chats/<sessão>.md`,
/// um item por mensagem. Devolve um resumo. Best-effort (sem transcript → avisa).
fn sync_chats(root: &Path, arch: &Path) -> String {
    let Some(td) = transcript_dir(root) else {
        return "chats: sem transcript do claude pra este projeto (nada a extrair)".into();
    };
    let chats = arch.join("chats");
    let _ = fs::create_dir_all(&chats);
    let mut sessions = 0usize;
    let mut msgs = 0usize;
    if let Ok(rd) = fs::read_dir(&td) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let sid = p.file_stem().and_then(|s| s.to_str()).unwrap_or("sessao").to_string();
            let (md, n) = transcript_to_md(&p);
            if n > 0 {
                let _ = fs::write(chats.join(format!("{sid}.md")), md);
                sessions += 1;
                msgs += n;
            }
        }
    }
    format!("chats: {sessions} sessão(ões), {msgs} mensagens em chats/")
}

/// Converte um transcript `.jsonl` num markdown de histórico (um item por mensagem: você / agente).
/// Retorna (markdown, nº de mensagens). Extração tolerante ao formato do claude.
fn transcript_to_md(path: &Path) -> (String, usize) {
    let Ok(txt) = fs::read_to_string(path) else {
        return (String::new(), 0);
    };
    let mut out = String::from("# Histórico de chat — evolução documentada\n\n");
    let mut n = 0usize;
    for line in txt.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // só as mensagens de conversa (type user/assistant com message.role/content).
        let msg = v.get("message");
        let role = msg
            .and_then(|m| m.get("role"))
            .and_then(|r| r.as_str())
            .or_else(|| v.get("type").and_then(|t| t.as_str()));
        let Some(role) = role else { continue };
        if role != "user" && role != "assistant" {
            continue;
        }
        let text = extract_text(msg.and_then(|m| m.get("content")));
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let who = if role == "user" { "Você" } else { "Agente" };
        out.push_str(&format!("## {who}\n\n{text}\n\n---\n\n"));
        n += 1;
    }
    (out, n)
}

/// Extrai o texto de um `content` do claude (string, ou array de blocos com `.text`).
fn extract_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text").and_then(|t| t.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Gera o próximo `context_agent/#N.txt` — o contexto PORTÁVEL do projeto (pra migrar entre PCs/apps):
/// objetivo, decisões, checklist aberto, estado. Numerado (não sobrescreve — cada gração é um snapshot).
fn write_context_agent(root: &Path, arch: &Path) -> Result<String, String> {
    let dir = arch.join("context_agent");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // próximo N (conta os #*.txt existentes).
    let n = fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.file_name().to_string_lossy().starts_with('#')
                        && e.path().extension().and_then(|s| s.to_str()) == Some("txt")
                })
                .count()
        })
        .unwrap_or(0)
        + 1;
    let od = crate::paths::overdev_dir_at(root);
    let read = |f: &str| fs::read_to_string(od.join(f)).unwrap_or_default();
    let proj = overdev::project_name(root);
    let objetivo = read("OBJETIVO.md");
    let decisoes = read("DECISOES.md");
    let plano = read("PLAN.md");
    let checklist = read("CHECKLIST.md");
    let abertos: Vec<&str> = checklist
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- [ ]") || t.starts_with("- [~]") || t.starts_with("- [H ]")
        })
        .collect();
    let body = format!(
        "# CONTEXTO PORTÁVEL — {proj} (context_agent/#{n})\n\
         gerado em {} (epoch)\n\n\
         Este arquivo carrega o CONTEXTO GERAL do projeto pra migrar entre PCs/apps. Cole no início\n\
         de uma nova sessão pra retomar de onde parou.\n\n\
         ## Objetivo\n{objetivo}\n\n\
         ## Decisões acordadas\n{decisoes}\n\n\
         ## Plano\n{plano}\n\n\
         ## Itens AINDA ABERTOS ({} aberto/on-hold)\n{}\n",
        util::now_unix(),
        abertos.len(),
        if abertos.is_empty() {
            "(nenhum item aberto registrado)".to_string()
        } else {
            abertos.join("\n")
        },
    );
    let path = dir.join(format!("#{n}.txt"));
    fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(format!("context_agent/#{n}.txt gerado (contexto portável)"))
}

/// Commita tudo no repo do archive (best-effort — nunca quebra o fluxo).
fn commit_archive(arch: &Path, msg: &str) {
    let run = |args: &[&str]| {
        let _ = std::process::Command::new("git").arg("-C").arg(arch).args(args).status();
    };
    run(&["add", "-A"]);
    // só commita se há mudança staged (git diff --cached --quiet → exit 1 se há diff).
    let has_change = std::process::Command::new("git")
        .arg("-C")
        .arg(arch)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false);
    if has_change {
        run(&["commit", "-q", "-m", msg]);
    }
}
