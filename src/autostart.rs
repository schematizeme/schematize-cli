//! Autostart do agente: vincula ao sistema via systemd user service (+ XDG fallback).
//! O quê: liga/desliga o `schematize agent` pra iniciar com a sessão do usuário.
//! Onde: `schematize autostart enable|disable`.

use crate::util::{self, home};
use std::fs;

const UNIT: &str = "schematize-agent.service";
const DESKTOP: &str = "schematize-agent.desktop";

fn systemd_dir() -> std::path::PathBuf {
    home().join(".config").join("systemd").join("user")
}
fn autostart_dir() -> std::path::PathBuf {
    home().join(".config").join("autostart")
}

/// O agente está vinculado ao sistema? (systemd is-active ou arquivos de autostart presentes.)
pub fn is_active() -> bool {
    if let Ok(out) = util::run("systemctl", &["--user", "is-active", UNIT]) {
        if out.trim() == "active" {
            return true;
        }
    }
    systemd_dir().join(UNIT).exists() || autostart_dir().join(DESKTOP).exists()
}

/// Registra e inicia o agente no login (systemd --user) + fallback XDG autostart.
pub fn enable(exe: &str) -> Result<(), String> {
    // 1) systemd user service (inicia com a sessão, reinicia se cair).
    let sd = systemd_dir();
    fs::create_dir_all(&sd).map_err(|e| e.to_string())?;
    let unit = format!(
        "[Unit]\nDescription=schematize — agente de atualização\nAfter=default.target\n\n\
         [Service]\nType=simple\nExecStart={exe} agent\nRestart=on-failure\nRestartSec=30\n\n\
         [Install]\nWantedBy=default.target\n"
    );
    fs::write(sd.join(UNIT), unit).map_err(|e| e.to_string())?;

    // 2) XDG autostart como cinto-e-suspensório (DEs sem systemd --user).
    let ad = autostart_dir();
    fs::create_dir_all(&ad).map_err(|e| e.to_string())?;
    let desk = format!(
        "[Desktop Entry]\nType=Application\nName=schematize agent\n\
         Comment=Checa atualizações das skills e do CLI\nExec={exe} agent\n\
         Terminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    fs::write(ad.join(DESKTOP), desk).map_err(|e| e.to_string())?;

    // 3) habilita + inicia agora (best-effort; não falha se systemd --user ausente).
    let _ = util::run("systemctl", &["--user", "daemon-reload"]);
    match util::run("systemctl", &["--user", "enable", "--now", UNIT]) {
        Ok(_) => println!("agente habilitado (systemd --user) + XDG autostart. Inicia com a sessão."),
        Err(e) => println!("XDG autostart criado; systemd --user não disponível ({e}). Inicia no próximo login."),
    }
    Ok(())
}

/// Para e remove o autostart do agente.
pub fn disable() -> Result<(), String> {
    let _ = util::run("systemctl", &["--user", "disable", "--now", UNIT]);
    let _ = fs::remove_file(systemd_dir().join(UNIT));
    let _ = fs::remove_file(autostart_dir().join(DESKTOP));
    let _ = util::run("systemctl", &["--user", "daemon-reload"]);
    println!("agente desabilitado e removido do autostart.");
    Ok(())
}
