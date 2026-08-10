//! `schematize status` — painel geral do ambiente.
//! O quê: versões (skills + CLI), agente, overdev, idioma e links, tudo num lugar.
//! Onde: chamado por main; também é a "home" conceitual da GUI.

use crate::i18n::{self, t, tf};
use crate::{autostart, links, overdev, registry, settings, skills};

fn header(s: &str) {
    println!("\n\x1b[1m{s}\x1b[0m");
}

/// Imprime o dashboard completo.
pub fn run() {
    println!("\x1b[1m{}\x1b[0m", t("status.title"));

    // Versões
    header(&t("status.section_versions"));
    let st = skills::load_state();
    for it in &registry::catalog() {
        println!("  {}", skills::status_line(it, &st, true));
    }
    let cur = env!("CARGO_PKG_VERSION");
    let cli_latest = skills::latest_release_tag("schematize-cli").unwrap_or_else(|| "?".into());
    let cli_up = if cli_latest == cur { t("common.current") } else { t("common.update") };
    println!("  {:<12} {:<8} latest={:<8} {}", t("status.cli"), cur, cli_latest, cli_up);

    // Agente
    header(&t("status.section_agent"));
    println!("  {}", if autostart::is_active() { t("status.agent_active") } else { t("status.agent_inactive") });

    // Overdev
    header(&t("status.section_overdev"));
    println!("  hooks: {}", if settings::overdev_enabled() { t("common.on") } else { t("common.off") });
    match overdev::status_brief() {
        (true, Some(obj)) => println!("  {}", tf("status.overdev_run_active", &[("obj", &obj)])),
        _ => println!("  {}", t("status.overdev_run_none")),
    }

    // Idioma
    header(&t("status.section_language"));
    let code = i18n::current_code();
    let name = i18n::name_of(&code).unwrap_or("English");
    println!("  {}", tf("lang.current", &[("code", &code), ("langname", name)]));

    // Links
    header(&t("status.section_links"));
    println!("  {:<8} {}", t("gui.site"), links::SITE);
    println!("  {:<8} {}", t("gui.blog"), links::BLOG);
    println!("  {:<8} {}", t("gui.github"), links::GITHUB);
}
