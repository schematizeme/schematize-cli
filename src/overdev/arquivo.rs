//! Ponte com o ARCHIVE do projeto: onde ele fica, o nome do projeto e o espelho
//! durável dos arquivos vivos do overdev.

use super::*;

/// `<projeto>/<projeto>_archive/` — dir de archive DENTRO do projeto (irmão dos microserviços),
/// nomeado com o NOME DO PROJETO (não só `_archive` — senão, sem separação por team, mistura os
/// archives de projetos diferentes). O archive é CRITICIDADE 0 (obrigatório): guarda checklists,
/// planos, decisões, histórico — a observabilidade da evolução. É um repo git próprio, privado.
pub fn archive_dir(root: &Path) -> Option<PathBuf> {
    Some(root.join(format!("{}_archive", project_name(root))))
}

/// Nome do PROJETO (a aplicação): o PREFIXO comum dos dirs de microserviço, que a casa nomeia como
/// `<projeto>_<microservice>` (ex.: `schematize_cli_rs`, `schematizeskills_api_rs` → projeto
/// "schematize"). Assim o archive fica `<projeto>_archive` mesmo quando a pasta do projeto tem outro
/// nome. Fallback: o basename do dir (projeto standalone, sem microserviços com prefixo comum).
pub fn project_name(root: &Path) -> String {
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let base = canon.file_name().and_then(|s| s.to_str()).unwrap_or("projeto").to_string();
    // sub-dirs "microserviço": não ocultos, não `*_archive`, e que seguem `<x>_<y>` (têm `_`).
    let mut names: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&canon) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                let n = e.file_name().to_string_lossy().to_string();
                if !n.starts_with('.') && !n.ends_with("_archive") && n.contains('_') {
                    names.push(n);
                }
            }
        }
    }
    if names.len() < 2 {
        return base; // standalone → basename do dir
    }
    names.sort();
    let first = names.first().cloned().unwrap_or_default();
    let last = names.last().cloned().unwrap_or_default();
    let mut common = String::new();
    for (a, b) in first.chars().zip(last.chars()) {
        if a == b {
            common.push(a);
        } else {
            break;
        }
    }
    let proj = common.trim_end_matches(['_', '-']).to_string();
    if proj.is_empty() {
        base
    } else {
        proj
    }
}

/// Materializa `<projeto>_archive/overdev/` e espelha os artefatos do overdev (OBJETIVO/PLAN/
/// DECISOES/CHECKLIST) que já existirem. Best-effort — nunca quebra o fluxo. Chamado no `start` e
/// no `check` (a cada tentativa de parada), pra o archive ficar sempre em dia conforme o agente escreve.
pub(crate) fn ensure_archive_mirror(root: &Path) {
    let Some(arch) = archive_dir(root) else { return };
    let od = arch.join("overdev");
    if fs::create_dir_all(&od).is_err() {
        return;
    }
    // O archive é um REPOSITÓRIO git PRÓPRIO (privado, obrigatório) que documenta a evolução do
    // projeto — irmão dos microserviços. Git-init se ainda não for repo + README. Best-effort.
    if !arch.join(".git").is_dir() {
        let _ =
            std::process::Command::new("git").arg("-C").arg(&arch).arg("init").arg("-q").status();
        let readme = arch.join("README.md");
        if !readme.exists() {
            let _ = fs::write(
                &readme,
                "# _archive — evolução documentada do projeto\n\nRepositório PRIVADO obrigatório (criticidade 0), irmão dos microserviços. Guarda o histórico\nDURÁVEL da evolução: `overdev/` (objetivo, plano, decisões, checklist, notas), `index/` (MAPA +\ngrafos), `decisoes/` (ADRs), `chats/` (handoffs FEITO-vs-ABERTO), `audit/`, `pentest/`. NÃO é\nopcional — é a observabilidade da evolução do sistema. Mantido em git privado, versionado por marco.\n",
            );
        }
    }
    let src = dir_at(root);
    for f in ["OBJETIVO.md", "PLAN.md", "DECISOES.md", "CHECKLIST.md", "NOTAS.md"] {
        let s = src.join(f);
        if s.is_file() {
            let _ = fs::copy(&s, od.join(f));
        }
    }
}

/// Comando que a GUI injeta na sessão do agente acoplado pra CARREGAR os preceitos
/// de engenharia da casa no contexto (slash command da skill de engenharia).
pub fn load_cmd() -> &'static str {
    "/eng-load"
}

/// Comando que a GUI injeta pra (re)INDEXAR o conteúdo do projeto (grafo/MAPA §39).
pub fn index_cmd() -> &'static str {
    "/eng-index"
}
