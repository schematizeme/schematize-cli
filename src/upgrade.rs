//! `schematize upgrade` — atualiza o próprio CLI/GUI pro latest.
//! O quê: compara versão atual vs release latest e roda o install.sh oficial.
//! Onde: chamado por main. Reusa o instalador (detecta distro, .deb/.rpm/binário).

use crate::i18n::{t, tf};
use crate::skills;
use crate::util;

const INSTALL_URL: &str =
    "https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh";

/// Checa e, se houver versão nova (ou `force`), roda o instalador.
pub fn run(force: bool) -> Result<(), String> {
    println!("{}", t("upgrade.checking"));
    let cur = env!("CARGO_PKG_VERSION");
    println!("{}", tf("upgrade.current", &[("v", cur)]));

    let latest = skills::latest_release_tag("schematize-cli");
    if let Some(l) = &latest {
        println!("{}", tf("upgrade.latest", &[("v", l)]));
    }

    let outdated = matches!(&latest, Some(l) if l != cur);
    if !outdated && !force {
        println!("{}", t("upgrade.uptodate"));
        return Ok(());
    }

    println!("{}", t("upgrade.available"));
    println!("{}", t("upgrade.running"));
    match util::run("bash", &["-c", &format!("curl -fsSL {INSTALL_URL} | bash")]) {
        Ok(out) => {
            print!("{out}");
            println!("{}", t("upgrade.done"));
            Ok(())
        }
        Err(e) => Err(tf("upgrade.failed", &[("e", &e)])),
    }
}
