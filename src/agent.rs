//! Agente de atualização: checa versões e notifica no desktop com botão "Atualizar".
//! O quê: roda residente (systemd user service) ou one-shot; usa dbus (notify-rust).
//! Onde: `schematize agent` (loop) e `schematize check [--notify]` (uma vez).

use crate::registry::{self, Item};
use crate::skills;
use crate::util;
use notify_rust::Notification;
use std::time::Duration;

/// Repo do próprio CLI (pra checar auto-atualização).
const CLI_REPO: &str = "schematize-cli";
const CLI_INSTALL_URL: &str =
    "https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh";

/// Uma atualização disponível.
pub struct Upd {
    pub name: String,
    pub installed: String,
    pub latest: String,
    pub item: Option<&'static Item>, // None = o próprio CLI
}

/// Resolve a última versão do CLI seguindo o redirect de um asset do release.
fn cli_latest() -> Option<String> {
    let url = format!("https://github.com/{}/{}/releases/latest/download/install.sh", registry::ORG, CLI_REPO);
    skills::version_from_redirect(&url)
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
        .summary("schematize — atualizado")
        .body(&format!("{} item(ns) atualizado(s).", ups.len()))
        .show();
}

/// Mostra a notificação com o botão Atualizar e trata o clique (bloqueia até ação/fechar).
fn notify(ups: &[Upd]) {
    let names: Vec<String> = ups.iter().map(|u| format!("{} {}→{}", u.name, u.installed, u.latest)).collect();
    let body = format!("{} atualização(ões):\n{}\n\n(ou rode: schematize update --all)", ups.len(), names.join("\n"));
    let res = Notification::new()
        .summary("schematize — atualizações disponíveis")
        .body(&body)
        .action("update", "Atualizar")
        .action("later", "Depois")
        .timeout(0)
        .show();
    match res {
        Ok(h) => h.wait_for_action(|a| {
            if a == "update" {
                apply(ups);
            }
        }),
        Err(e) => eprintln!("(notificação indisponível: {e}) — rode: schematize update --all"),
    }
}

/// One-shot: imprime o status; com `do_notify`, dispara a notificação se houver att.
pub fn run_once(do_notify: bool) {
    let ups = check();
    if ups.is_empty() {
        println!("tudo atualizado.");
        return;
    }
    println!("{} atualização(ões) disponível(is):", ups.len());
    for u in &ups {
        println!("  {} {} → {}", u.name, u.installed, u.latest);
    }
    if do_notify {
        notify(&ups);
    } else {
        println!("rode: schematize update --all  (ou schematize check --notify)");
    }
}

/// Loop residente: checa a cada intervalo (default 6h) e notifica.
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
        std::thread::sleep(Duration::from_secs(secs));
    }
}
