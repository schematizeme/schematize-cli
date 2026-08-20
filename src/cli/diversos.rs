//! Subcomandos avulsos: governador de agentes, idioma, relatório de debug,
//! projetos e o log do git.

use schematize::i18n::{t, tf};
use schematize::{
    config, debugreport, githist, projects,
};
use schematize::i18n;
use schematize::debug;
use crate::cli::args::*;
use crate::cli::ssh::canon_or;

/// `schematize lang [code] [--list]`.
/// `schematize agents` — imprime o orçamento de concorrência e persiste ~/.schematize/agents.json.
pub(crate) fn agents_cmd(json: bool, split: Option<usize>) -> Result<(), String> {
    let b = schematize::agents::budget();
    let _ = schematize::agents::persist(&b); // best-effort: outros (Claude/overdev/GUI) leem daqui.

    if json {
        let plan = split.map(|k| b.split_plan(k));
        let mut v = serde_json::json!({
            "total_cap": b.total_cap, "available": b.available,
            "cpu_cap": b.cpu_cap, "ram_cap": b.ram_cap, "load_cap": b.load_cap,
            "threads": b.snap.threads, "mem_available_mb": b.snap.mem_available_mb,
            "load1": b.snap.load1, "running_claudes": b.snap.running_claudes,
            "ram_tight": b.ram_tight,
        });
        if let Some(p) = plan {
            v["split"] = serde_json::json!({
                "mains": p.mains, "subagents_each": p.subagents_each, "total_used": p.total_used
            });
        }
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(());
    }

    let gb = |mb: u64| format!("{:.1} GB", mb as f64 / 1024.0);
    println!("\x1b[1mOrçamento de concorrência do Claude (máquina inteira)\x1b[0m");
    println!("  threads lógicos      : {}", b.snap.threads);
    println!("  reserva (respiro)    : {}", b.params.reserve);
    println!("  RAM disponível       : {}  (≈{} por agent, −{:.0}% de margem)", gb(b.snap.mem_available_mb), gb(b.params.mb_per_agent), b.params.ram_margin * 100.0);
    println!("  load atual (1min)    : {:.2}", b.snap.load1);
    println!("  claudes rodando AGORA: {}  (esta janela + outras + subagents)", b.snap.running_claudes);
    println!("  ─────────────────────");
    println!("  teto por CPU         : {}", b.cpu_cap);
    println!("  teto por RAM         : {}{}", b.ram_cap, if b.ram_tight { "  \x1b[33m(RAM apertada — cuidado com swap)\x1b[0m" } else { "" });
    println!("  teto por load        : {}", b.load_cap);
    println!("  \x1b[1mTETO TOTAL seguro    : {}\x1b[0m  (o menor dos três)", b.total_cap);
    println!("  \x1b[1;32mDISPONÍVEL p/ lançar : {}\x1b[0m  (teto − já rodando)", b.available);
    if let Some(k) = split {
        let p = b.split_plan(k);
        println!("\n  \x1b[1mSplit em {} claude(s) principal(is):\x1b[0m {} subagents cada  (total {}/{} do teto)", p.mains, p.subagents_each, p.total_used, p.cap);
    }
    Ok(())
}

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
/// Sem `--collect`: o debug do updater (comportamento atual). Com `--collect`: monta o
/// relatório completo (secret-safe) e grava um arquivo modo 600 (ou imprime com `--stdout`).
pub(crate) fn debug_cmd(collect: bool, out: Option<String>, stdout: bool, online: bool) -> Result<(), String> {
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

/// `schematize git-log [--limit N]` — commits recentes marcando push (●/○).
pub(crate) fn git_log(limit: usize) {
    let root = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cwd inacessível: {e}");
            return;
        }
    };
    let cs = githist::commits(&root, limit);
    if cs.is_empty() {
        println!("sem commits (ou não é um repositório git).");
        return;
    }
    for c in &cs {
        let mark = if c.pushed { '●' } else { '○' };
        println!("{mark} {}  {:<10}  {}  {}", c.short, c.date, c.author, c.subject);
    }
    match githist::upstream(&root) {
        Some(u) => println!(
            "\nbranch {} → {} (ahead {}, behind {})  [● pushado · ○ local]",
            u.branch,
            u.remote.as_deref().unwrap_or("?"),
            u.ahead,
            u.behind
        ),
        None => println!("\nbranch sem upstream (nenhum commit pushado)  [○ local]"),
    }
}

/// `schematize projects <sub>` — lista/fixa/marca projetos.
pub(crate) fn projects_cmd(sub: ProjectsCmd) -> Result<(), String> {
    match sub {
        ProjectsCmd::List => {
            let dev_dirs = config::dev_dirs();
            let pinned = config::projects();
            let projs = projects::scan_with_pins(&dev_dirs, &pinned);
            if projs.is_empty() {
                println!("Nenhum projeto encontrado (cadastre dev_dirs ou fixe com `projects add`).");
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
            let marker = std::path::Path::new(&dir).join(".schematize");
            if marker.exists() {
                std::fs::remove_file(&marker).map_err(|e| format!("falha ao remover marcador: {e}"))?;
                println!("Marcador removido: {}", marker.display());
            } else {
                println!("Sem marcador em {}", marker.display());
            }
            Ok(())
        }
    }
}
