//! `schematize upgrade` — atualiza o próprio CLI/GUI pro latest.
//! O quê: compara versão atual vs release latest e troca os binários SEM sudo
//! (via selfupdate: dir do exe se gravável, senão pkexec, senão ~/.local/bin).
//! Onde: chamado por main. O install.sh (com sudo) fica só como caminho manual.

use crate::i18n::{t, tf};
use crate::{selfupdate, skills};

/// Checa e, se houver versão nova (ou `force`), troca os binários.
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
    match selfupdate::run() {
        Ok(msg) => {
            println!("{msg}");
            println!("{}", t("upgrade.done"));
            Ok(())
        }
        Err(e) => Err(tf("upgrade.failed", &[("e", &e)])),
    }
}
