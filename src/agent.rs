//! Agente de atualização: checa versões e notifica no desktop com botão "Atualizar".
//! O quê: roda residente (systemd user service) ou one-shot; usa dbus (notify-rust).
//! Onde: `schematize agent` (loop) e `schematize check [--notify]` (uma vez).

use crate::i18n::{t, tf};
use crate::registry::{self, Item};
use crate::{news, skills, util};
use notify_rust::Notification;
use std::time::Duration;

/// Repo do próprio CLI (pra checar auto-atualização).
const CLI_REPO: &str = "schematize-cli";

/// Última versão publicada do CLI (via API do GitHub).
fn cli_latest() -> Option<String> {
    skills::latest_release_tag(CLI_REPO)
}

/// Uma atualização disponível.
pub struct Upd {
    pub name: String,
    pub installed: String,
    pub latest: String,
    pub item: Option<Item>, // None = o próprio CLI
}

/// Lista o que tem atualização: skills instaladas desatualizadas + o próprio CLI.
/// O conjunto de skills vem do ÍNDICE REMOTO (registry::catalog) — pega skills novas.
pub fn check() -> Vec<Upd> {
    let mut out = Vec::new();
    for it in registry::catalog() {
        // fonte de verdade = VERSION no disco (funciona mesmo instalada por install.sh).
        if let Some(inst) = skills::installed_version(&it) {
            if let Ok(latest) = skills::resolve_latest(&it) {
                if inst != latest {
                    out.push(Upd { name: it.slug.clone(), installed: inst, latest, item: Some(it) });
                }
            }
        }
    }
    let cur = env!("CARGO_PKG_VERSION");
    if let Some(latest) = cli_latest() {
        if latest != cur {
            out.push(Upd { name: "schematize (CLI)".into(), installed: cur.into(), latest, item: None });
        }
    }
    out
}

/// Aplica todas as atualizações (skills via install; CLI via bootstrap).
fn apply(ups: &[Upd]) {
    let mut ok = 0usize;
    let mut errs: Vec<String> = Vec::new();
    for u in ups {
        match &u.item {
            // Skill: instalação in-process (user-space, sem root) — já funcionava.
            Some(it) => match skills::install(it) {
                Ok(_) => ok += 1,
                Err(e) => errs.push(format!("{}: {e}", u.name)),
            },
            // CLI/GUI: self-update SEM sudo (a correção do "não atualiza") + resultado real.
            None => match crate::selfupdate::run() {
                Ok(_) => ok += 1,
                Err(e) => errs.push(format!("schematize CLI: {e}")),
            },
        }
    }
    // Notificação de RESULTADO honesta: sucesso e/ou falha (com motivo), nunca só "pronto".
    let mut n = Notification::new();
    n.icon("schematize");
    if errs.is_empty() {
        n.summary(&t("agent.updated_title"))
            .body(&tf("agent.n_updated", &[("n", &ok.to_string())]));
    } else {
        let body = format!(
            "{}\n{}",
            tf("agent.n_updated", &[("n", &ok.to_string())]),
            errs.join("\n"),
        );
        n.summary(&t("agent.update_failed")).body(&body);
    }
    let _ = n.timeout(0).show();
}

/// Mostra a notificação com o botão Atualizar e trata o clique (bloqueia até ação/fechar).
fn notify(ups: &[Upd]) {
    let names: Vec<String> = ups.iter().map(|u| format!("{} {}→{}", u.name, u.installed, u.latest)).collect();
    let body = format!("{}\n{}\n\n{}", tf("agent.n_updates", &[("n", &ups.len().to_string())]), names.join("\n"), t("agent.hint"));
    let res = Notification::new()
        .summary(&t("agent.updates_available"))
        .body(&body)
        .icon("schematize")
        .action("update", &t("agent.btn_update"))
        .action("later", &t("agent.btn_later"))
        .timeout(0)
        .show();
    match res {
        Ok(h) => h.wait_for_action(|a| {
            if a == "update" {
                apply(ups);
            }
        }),
        Err(e) => eprintln!("{}", tf("agent.unavailable", &[("e", &e.to_string())])),
    }
}

/// Notifica um post novo do blog com ação de abrir no navegador.
fn notify_blog(link: &str) {
    let res = Notification::new()
        .summary(&tf("news.new_posts", &[("n", "1")]))
        .body(&tf("agent.new_posts_body", &[("url", link)]))
        .icon("schematize")
        .action("open", &t("gui.blog"))
        .timeout(0)
        .show();
    if let Ok(h) = res {
        h.wait_for_action(|a| {
            if a == "open" {
                util::open_url(link);
            }
        });
    }
}

/// One-shot: imprime o status; com `do_notify`, dispara a notificação se houver att.
pub fn run_once(do_notify: bool) {
    let ups = check();
    if ups.is_empty() {
        println!("{}", t("agent.all_uptodate"));
    } else {
        println!("{}", tf("agent.n_updates", &[("n", &ups.len().to_string())]));
        for u in &ups {
            println!("  {} {} → {}", u.name, u.installed, u.latest);
        }
        if do_notify {
            notify(&ups);
        } else {
            println!("{}", t("agent.hint"));
        }
    }
    // Blog: novidade desde a última checagem.
    if let Some(link) = news::check_new() {
        if do_notify {
            notify_blog(&link);
        } else {
            println!("{}", tf("news.new_posts", &[("n", "1")]));
            println!("  {link}");
        }
    }
}

/// Loop residente: checa a cada intervalo (default 6h) e notifica (updates + blog).
pub fn run_loop() {
    let secs: u64 = std::env::var("SCHEMATIZE_CHECK_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6 * 3600);
    loop {
        let ups = check();
        if !ups.is_empty() {
            notify(&ups);
        }
        if let Some(link) = news::check_new() {
            notify_blog(&link);
        }
        std::thread::sleep(Duration::from_secs(secs));
    }
}
