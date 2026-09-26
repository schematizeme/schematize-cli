//! `schematize status` — painel geral do ambiente.
//! O quê: versões (skills + CLI), agente, overdev, idioma e links, tudo num lugar.
//! Onde: chamado por main; também é a "home" conceitual da GUI.

use crate::i18n::{self, t, tf};
use crate::versoes;
use crate::{autostart, links, overdev, settings};

fn header(s: &str) {
    println!("\n\x1b[1m{s}\x1b[0m");
}

/// Imprime o dashboard completo.
pub fn run() {
    println!("\x1b[1m{}\x1b[0m", t("status.title"));

    // Versões
    header(&t("status.section_versions"));
    // O catálogo vem do APP (E5). A frase de estado é montada AQUI porque é texto de tela —
    // o app devolve o slug (`em_dia`, `tem_atualizacao`…), e quem imprime escolhe a palavra.
    for s in crate::skillslink::catalogo() {
        let estado = match s.situacao.as_str() {
            "em_dia" => i18n::t("common.current"),
            "tem_atualizacao" => i18n::t("common.update"),
            "nao_instalada" => i18n::t("common.not_installed"),
            "fork" => "[fork]".to_string(),
            _ => String::new(),
        };
        let inst = if s.instalada.is_empty() { "—" } else { &s.instalada };
        let ult = if s.ultima.is_empty() { "?" } else { &s.ultima };
        println!("  {:<12} {:<8} latest={:<8} {estado}", s.slug, inst, ult);
    }
    let cur = env!("CARGO_PKG_VERSION");
    let cli_latest = versoes::latest_version_raw("schematize-cli").unwrap_or_else(|| "?".into());
    let cli_up = if cli_latest == cur { t("common.current") } else { t("common.update") };
    println!("  {:<12} {:<8} latest={:<8} {}", t("status.cli"), cur, cli_latest, cli_up);

    // Agente
    header(&t("status.section_agent"));
    println!(
        "  {}",
        if autostart::is_active() { t("status.agent_active") } else { t("status.agent_inactive") }
    );

    // Overdev
    header(&t("status.section_overdev"));
    println!(
        "  hooks: {}",
        if settings::overdev_enabled() { t("common.on") } else { t("common.off") }
    );
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
