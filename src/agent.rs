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
const CLI_INSTALL_URL: &str =
    "https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh";

/// Última versão publicada do CLI (via API do GitHub).
fn cli_latest() -> Option<String> {
    skills::latest_release_tag(CLI_REPO)
}

/// Uma atualização disponível.
pub struct Upd {
    pub name: String,
    pub installed: String,
    pub latest: String,
    pub item: Option<&'static Item>, // None = o próprio CLI
}

/// Lista o que tem atualização: skills instaladas desatualizadas + o próprio CLI.
pub fn check() -> Vec<Upd> {
    let mut out = Vec::new();
    for it in registry::ITEMS {
        // fonte de verdade = VERSION no disco (funciona mesmo instalada por install.sh).
        if let Some(inst) = skills::installed_version(it) {
            if let Ok(latest) = skills::resolve_latest(it) {
                if inst != latest {
                    out.push(Upd { name: it.slug.into(), installed: inst, latest, item: Some(it) });
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
    for u in ups {
        match u.item {
            Some(it) => {
                let _ = skills::install(it);
            }
            None => {
                let _ = util::run("bash", &["-c", &format!("curl -fsSL {CLI_INSTALL_URL} | bash")]);
            }
        }
    }
    let _ = Notification::new()
        .summary(&t("agent.updated_title"))
        .body(&tf("agent.n_updated", &[("n", &ups.len().to_string())]))
        .show();
}

/// Mostra a notificação com o botão Atualizar e trata o clique (bloqueia até ação/fechar).
fn notify(ups: &[Upd]) {
    let names: Vec<String> = ups.iter().map(|u| format!("{} {}→{}", u.name, u.installed, u.latest)).collect();
    let body = format!("{}\n{}\n\n{}", tf("agent.n_updates", &[("n", &ups.len().to_string())]), names.join("\n"), t("agent.hint"));
    let res = Notification::new()
        .summary(&t("agent.updates_available"))
        .body(&body)
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
