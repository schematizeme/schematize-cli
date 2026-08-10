//! `schematize doctor` — diagnóstico do ambiente + reparo opcional (--fix).
//! O quê: checa ~/.claude, skills dir, settings.json, hooks, agente, PATH e rede.
//! Onde: chamado por main. Não toca em nada sem --fix (além de criar diretórios base).

use crate::i18n::{t, tf};
use crate::{autostart, settings, skills, util};
use std::fs;
use std::path::Path;

/// Severidade de um check.
enum Lv {
    Ok,
    Warn,
    Fail,
}

/// Imprime uma linha de check já traduzida.
fn line(lv: &Lv, label: &str, detail: &str) {
    let tag = match lv {
        Lv::Ok => format!("\x1b[1;32m{}\x1b[0m", t("doctor.ok")),
        Lv::Warn => format!("\x1b[1;33m{}\x1b[0m", t("doctor.warn")),
        Lv::Fail => format!("\x1b[1;31m{}\x1b[0m", t("doctor.fail")),
    };
    if detail.is_empty() {
        println!("  [{tag}] {label}");
    } else {
        println!("  [{tag}] {label} — {detail}");
    }
}

/// Roda o diagnóstico. Com `fix`, cria diretórios/repara o que for seguro.
pub fn run(fix: bool) {
    println!("{}\n", t("doctor.title"));
    let mut issues = 0usize;

    // ~/.claude
    let cdir = util::claude_dir();
    if cdir.is_dir() {
        line(&Lv::Ok, &t("doctor.check_claude_dir"), "");
    } else if fix && fs::create_dir_all(&cdir).is_ok() {
        line(&Lv::Ok, &t("doctor.check_claude_dir"), &t("doctor.fixed"));
    } else {
        issues += 1;
        line(&Lv::Warn, &t("doctor.check_claude_dir"), "");
    }

    // skills dir
    let sdir = util::skills_dir();
    if sdir.is_dir() {
        line(&Lv::Ok, &t("doctor.check_skills_dir"), "");
    } else if fix && fs::create_dir_all(&sdir).is_ok() {
        line(&Lv::Ok, &t("doctor.check_skills_dir"), &t("doctor.fixed"));
    } else {
        issues += 1;
        line(&Lv::Warn, &t("doctor.check_skills_dir"), "");
    }

    // settings.json válido
    match settings::settings_valid() {
        Some(true) | None => line(&Lv::Ok, &t("doctor.check_settings"), ""),
        Some(false) => {
            issues += 1;
            line(&Lv::Fail, &t("doctor.check_settings"), "");
        }
    }

    // overdev hooks (informativo)
    let od = if settings::overdev_enabled() { t("common.on") } else { t("common.off") };
    line(&Lv::Ok, &t("doctor.check_overdev"), &od);

    // agente (informativo)
    let ag = if autostart::is_active() { t("status.agent_active") } else { t("status.agent_inactive") };
    line(&Lv::Ok, &t("doctor.check_agent"), &ag);

    // PATH shadow
    match shadowed() {
        Some(p) => {
            issues += 1;
            line(&Lv::Warn, &t("doctor.check_path"), &tf("doctor.path_shadow", &[("path", &p)]));
        }
        None => line(&Lv::Ok, &t("doctor.check_path"), ""),
    }

    // rede (GitHub)
    if github_reachable() {
        line(&Lv::Ok, &t("doctor.check_network"), "");
    } else {
        issues += 1;
        line(&Lv::Fail, &t("doctor.check_network"), "");
    }

    // versão do CLI vs latest
    let cur = env!("CARGO_PKG_VERSION");
    match skills::latest_release_tag("schematize-cli") {
        Some(l) if l != cur => {
            issues += 1;
            line(&Lv::Warn, &t("doctor.check_cli"), &format!("{cur} → {l}"));
        }
        _ => line(&Lv::Ok, &t("doctor.check_cli"), cur),
    }

    println!();
    if issues == 0 {
        println!("\x1b[1;32m{}\x1b[0m", t("doctor.summary_ok"));
    } else {
        println!("\x1b[1;33m{}\x1b[0m", tf("doctor.summary_issues", &[("n", &issues.to_string())]));
    }
}

/// Retorna o caminho do binário que sombreia /usr/bin/schematize, se houver.
fn shadowed() -> Option<String> {
    let bin = util::run("bash", &["-lc", "command -v schematize"]).ok()?;
    let bin = bin.trim().to_string();
    if !bin.is_empty() && bin != "/usr/bin/schematize" && Path::new("/usr/bin/schematize").exists() {
        Some(bin)
    } else {
        None
    }
}

/// GitHub acessível? (HEAD rápido na API, sem baixar corpo.)
fn github_reachable() -> bool {
    util::run("curl", &["-sfI", "-m", "6", "https://api.github.com"]).is_ok()
}
