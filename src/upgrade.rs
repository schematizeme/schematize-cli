//! `schematize upgrade` — RECOMPILA o CLI/GUI do fonte (source-first).
//! O quê: puxa o install.sh do main e roda em modo --from-source (rustup + libs de
//! build + `cargo install --features gui`), com stdio herdado pra sudo/rustup poderem
//! pedir no terminal. Não depende de binário publicado por CI.
//! Onde: chamado por main (`schematize upgrade`). A GUI/agente têm o atalho binário
//! (selfupdate.rs) pra quando existir release pronto; este é o caminho de verdade.

use crate::i18n::{t, tf};
use std::process::Command;

const INSTALL_SH: &str = "https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh";

/// Repo GitHub do PRÓPRIO app (a CLI/GUI) — usado pra resolver a versão mais nova do app.
const APP_REPO: &str = "schematize-cli";

/// Versão do app compilada (do Cargo.toml). Fonte de verdade do "estou na vX".
/// Onde: exibida no status/GUI e comparada com a última publicada em `app_update_available`.
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Há versão mais nova do PRÓPRIO app publicada? Retorna `Some((atual, latest))` se sim, `None`
/// se já está em dia OU se a checagem falhou (rede/GitHub) — resiliente, nunca panica.
/// O quê: reusa o MESMO mecanismo raw de `skills::latest_version_raw` (lê a versão do `Cargo.toml`
/// no `main` via raw.githubusercontent, SEM a API 60/h) e compara semver com `util::semver_lt`.
/// Onde: consumido pela GUI (badge "Atualizar app") e pelo módulo de notificações.
pub fn app_update_available() -> Option<(String, String)> {
    let cur = app_version().to_string();
    let latest = crate::skills::latest_version_raw(APP_REPO)?;
    if crate::util::semver_lt(&cur, &latest) {
        Some((cur, latest))
    } else {
        None
    }
}

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
