//! `schematize upgrade` — RECOMPILA o CLI/GUI do fonte (source-first).
//! O quê: puxa o install.sh do main e roda em modo --from-source (rustup + libs de
//! build + `cargo install --features gui`), com stdio herdado pra sudo/rustup poderem
//! pedir no terminal. Não depende de binário publicado por CI.
//! Onde: chamado por main (`schematize upgrade`). A GUI/agente têm o atalho binário
//! (selfupdate.rs) pra quando existir release pronto; este é o caminho de verdade.

use crate::i18n::{t, tf};
use std::process::Command;

const INSTALL_SH: &str = "https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh";

/// Recompila do fonte (sempre pega o main). `force` é aceito por compat (upgrade já reconstrói).
pub fn run(_force: bool) -> Result<(), String> {
    println!("{}", t("upgrade.checking"));
    println!("{}", tf("upgrade.current", &[("v", env!("CARGO_PKG_VERSION"))]));
    println!("{}", t("upgrade.running"));
    // stdio herdado: sudo (libs de build) e rustup podem pedir no terminal.
    let status = Command::new("bash")
        .arg("-c")
        .arg(format!("curl -fsSL {INSTALL_SH} | bash -s -- --from-source"))
        .status()
        .map_err(|e| tf("upgrade.failed", &[("e", &e.to_string())]))?;
    if status.success() {
        println!("{}", t("upgrade.done"));
        Ok(())
    } else {
        Err(tf("upgrade.failed", &[("e", "instalador do fonte falhou")]))
    }
}
