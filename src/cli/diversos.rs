//! Subcomandos avulsos: governador de agentes, idioma, relatório de debug,
//! projetos e o log do git.

use crate::cli::args::*;
use crate::cli::prompt::canon_or;
use schematize::debug;
use schematize::i18n;
use schematize::i18n::{t, tf};
use schematize::{config, debugreport, projects};

// O `agents_cmd` foi com o overdev (E6): o governador de concorrência existe para o SPLIT
// saber quantos agentes cabem antes de lançar, e quem faz split é o app do overdev.
// `schematize agents` continua funcionando — o `applink` o encaminha.

pub(crate) fn lang_cmd(code: Option<String>, list: bool) -> Result<(), String> {
    if list {
        println!("{}", t("lang.available"));
        for (c, name, _) in i18n::LANGS {
            println!("  {c:<4} {name}");
        }
        return Ok(());
    }
    match code {
        Some(c) => {
            if !i18n::is_supported(&c) {
                return Err(tf("lang.unknown", &[("code", &c)]));
            }
            i18n::set_lang(&c)?;
            let name = i18n::name_of(&c).unwrap_or("");
            println!("{}", tf("lang.set", &[("code", &c), ("langname", name)]));
            println!("{}", t("lang.restart_gui"));
            Ok(())
        }
        None => {
            let c = i18n::current_code();
            let name = i18n::name_of(&c).unwrap_or("");
            println!("{}", tf("lang.current", &[("code", &c), ("langname", name)]));
            Ok(())
        }
    }
}

/// `schematize debug [--collect] [--out <path>] [--stdout]`.
/// Sem `--collect`: o debug do gestor de atualizações (comportamento atual). Com
/// `--collect`: monta o
/// relatório completo (secret-safe) e grava um arquivo modo 600 (ou imprime com `--stdout`).
pub(crate) fn debug_cmd(
    collect: bool,
    out: Option<String>,
    stdout: bool,
    online: bool,
) -> Result<(), String> {
    if !collect {
        debug::run();
        return Ok(());
    }
    if stdout {
        print!("{}", debugreport::collect(online));
        return Ok(());
    }
    let path = debugreport::write_report(out.as_deref().map(std::path::Path::new), online)?;
    println!("Relatório de debug gravado em: {}", path.display());
    println!("  {}", debugreport::short_summary());
    println!("  (modo 600 — segredos redigidos automaticamente; revise antes de compartilhar.)");
    Ok(())
}

// O `git_log` saiu daqui com o domínio (E2 da extradição, ADR-0019). Ele era o gêmeo de
// topo do `schematize git log`, e os dois liam o mesmo `githist` — duas superfícies para a
// mesma pergunta. Agora `schematize git-log` é interceptado pelo `applink` e vira
// `schematize-git log`, com o prefixo TROCADO (o campo `vira`) em vez de consumido.

/// `schematize projects <sub>` — lista/fixa/marca projetos.
pub(crate) fn projects_cmd(sub: ProjectsCmd) -> Result<(), String> {
    match sub {
        ProjectsCmd::List => {
            let dev_dirs = config::dev_dirs();
            let pinned = config::projects();
            let projs = projects::scan_with_pins(&dev_dirs, &pinned);
            if projs.is_empty() {
                println!(
                    "Nenhum projeto encontrado (cadastre dev_dirs ou fixe com `projects add`)."
                );
                return Ok(());
            }
            println!("Projetos ({}):", projs.len());
            for p in &projs {
                let flag = if p.marker == "pinned" { "[fixado] " } else { "" };
                println!("  {flag}{}  {}  ({})", p.name, p.path, p.marker);
            }
            Ok(())
        }
        ProjectsCmd::Add { path } => {
            let canon = canon_or(&path);
            config::pin_project(&path);
            println!("Fixado: {canon}");
            Ok(())
        }
        ProjectsCmd::Remove { path } => {
            config::unpin_project(&path);
            println!("Desafixado: {}", canon_or(&path));
            Ok(())
        }
        ProjectsCmd::Mark { path } => {
            let dir = path.unwrap_or_else(|| ".".to_string());
            let dir = canon_or(&dir);
            let marker = std::path::Path::new(&dir).join(".schematize");
            std::fs::write(&marker, "{}\n").map_err(|e| format!("falha ao criar marcador: {e}"))?;
            println!("Marcado como projeto: {}", marker.display());
            Ok(())
        }
        ProjectsCmd::Unmark { path } => {
            let dir = path.unwrap_or_else(|| ".".to_string());
            let dir = canon_or(&dir);
            // Desmarcar tem de achar o marcador com QUALQUER um dos nomes — senão
            // um projeto marcado antes do rename fica impossível de desmarcar.
            let base = std::path::Path::new(&dir);
            let marker = [".schematize", ".overflow"]
                .iter()
                .map(|n| base.join(n))
                .find(|p| p.exists())
                .unwrap_or_else(|| base.join(".schematize"));
            if marker.exists() {
                std::fs::remove_file(&marker)
                    .map_err(|e| format!("falha ao remover marcador: {e}"))?;
                println!("Marcador removido: {}", marker.display());
            } else {
                println!("Sem marcador em {}", marker.display());
            }
            Ok(())
        }
    }
}
