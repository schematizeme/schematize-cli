//! `schematize doctor` — diagnóstico do ambiente + reparo opcional (--fix).
//! O quê: checa ~/.claude, skills dir, settings.json, hooks, agente, PATH e rede.
//! Onde: chamado por main. Não toca em nada sem --fix (além de criar diretórios base).

use crate::i18n::{t, tf};
use crate::{autostart, settings, skills, updaterboot, util};
use std::fs;
use std::path::Path;

/// Severidade de um check.
enum Lv {
    Ok,
    Warn,
    Fail,
}

/// Imprime uma linha de check já traduzida.
fn line(lv: &Lv, label: &str, detail: &str) {
    let tag = match lv {
        Lv::Ok => format!("\x1b[1;32m{}\x1b[0m", t("doctor.ok")),
        Lv::Warn => format!("\x1b[1;33m{}\x1b[0m", t("doctor.warn")),
        Lv::Fail => format!("\x1b[1;31m{}\x1b[0m", t("doctor.fail")),
    };
    if detail.is_empty() {
        println!("  [{tag}] {label}");
    } else {
        println!("  [{tag}] {label} — {detail}");
    }
}

/// Roda o diagnóstico. Com `fix`, cria diretórios/repara o que for seguro.
pub fn run(fix: bool) {
    println!("{}\n", t("doctor.title"));
    let mut issues = 0usize;

    // ~/.claude
    let cdir = util::claude_dir();
    if cdir.is_dir() {
        line(&Lv::Ok, &t("doctor.check_claude_dir"), "");
    } else if fix && fs::create_dir_all(&cdir).is_ok() {
        line(&Lv::Ok, &t("doctor.check_claude_dir"), &t("doctor.fixed"));
    } else {
        issues += 1;
        line(&Lv::Warn, &t("doctor.check_claude_dir"), "");
    }

    // skills dir
    let sdir = util::skills_dir();
    if sdir.is_dir() {
        line(&Lv::Ok, &t("doctor.check_skills_dir"), "");
    } else if fix && fs::create_dir_all(&sdir).is_ok() {
        line(&Lv::Ok, &t("doctor.check_skills_dir"), &t("doctor.fixed"));
    } else {
        issues += 1;
        line(&Lv::Warn, &t("doctor.check_skills_dir"), "");
    }

    // settings.json válido
    match settings::settings_valid() {
        Some(true) | None => line(&Lv::Ok, &t("doctor.check_settings"), ""),
        Some(false) => {
            issues += 1;
            line(&Lv::Fail, &t("doctor.check_settings"), "");
        }
    }

    // overdev hooks (informativo)
    let od = if settings::overdev_enabled() { t("common.on") } else { t("common.off") };
    line(&Lv::Ok, &t("doctor.check_overdev"), &od);

    // agente (informativo)
    let ag = if autostart::is_active() { t("status.agent_active") } else { t("status.agent_inactive") };
    line(&Lv::Ok, &t("doctor.check_agent"), &ag);

    // PATH shadow
    match shadowed() {
        Some(p) => {
            issues += 1;
            line(&Lv::Warn, &t("doctor.check_path"), &tf("doctor.path_shadow", &[("path", &p)]));
        }
        None => line(&Lv::Ok, &t("doctor.check_path"), ""),
    }

    // rede (GitHub)
    if github_reachable() {
        line(&Lv::Ok, &t("doctor.check_network"), "");
    } else {
        issues += 1;
        line(&Lv::Fail, &t("doctor.check_network"), "");
    }

    // versão do CLI vs latest
    let cur = env!("CARGO_PKG_VERSION");
    match skills::latest_version_raw("schematize-cli") {
        Some(l) if l != cur => {
            issues += 1;
            line(&Lv::Warn, &t("doctor.check_cli"), &format!("{cur} → {l}"));
        }
        _ => line(&Lv::Ok, &t("doctor.check_cli"), cur),
    }

    // GUI: flavor (Slint novo vs egui antigo), lançador .desktop e coexistência pacote+fonte.
    issues += gui_checks(fix);

    // Gestor de atualizações (`schematize-updater`). Sem ele o update cai no
    // fluxo interno — que é justamente o caminho de "cliquei e não aconteceu
    // nada". O doctor INSTALA (não só reclama): é o que o usuário espera de um
    // comando chamado "doctor", e não exige saber que existe um gestor separado.
    if updaterboot::present() {
        line(&Lv::Ok, "gestor de atualizações (schematize-updater)", "");
    } else {
        match updaterboot::ensure_now() {
            updaterboot::Outcome::Instalado(p) => {
                line(&Lv::Ok, "gestor de atualizações (schematize-updater)", &format!("instalado em {}", p.display()));
            }
            updaterboot::Outcome::JaTinha => {
                line(&Lv::Ok, "gestor de atualizações (schematize-updater)", "");
            }
            outcome => {
                issues += 1;
                let detalhe = match outcome {
                    updaterboot::Outcome::Falhou(e) => e,
                    _ => "tentativa recente falhou — tento de novo mais tarde".to_string(),
                };
                line(&Lv::Warn, "gestor de atualizações (schematize-updater)", &detalhe);
            }
        }
    }

    println!();
    if issues == 0 {
        println!("\x1b[1;32m{}\x1b[0m", t("doctor.summary_ok"));
    } else {
        println!("\x1b[1;33m{}\x1b[0m", tf("doctor.summary_issues", &[("n", &issues.to_string())]));
    }
}

/// Versão TEXTO (sem ANSI, sem --fix) dos checks do doctor — reusada pelo coletor de
/// debug (`debugreport`). Só LÊ o ambiente (nunca repara). Best-effort: nunca panica.
pub fn report_text() -> String {
    use std::fmt::Write as _;
    let mut o = String::new();
    let ln = |lv: &str, label: &str, detail: &str, o: &mut String| {
        if detail.is_empty() {
            let _ = writeln!(o, "[{lv}] {label}");
        } else {
            let _ = writeln!(o, "[{lv}] {label} — {detail}");
        }
    };

    // ~/.claude e skills dir.
    ln(if util::claude_dir().is_dir() { "OK" } else { "WARN" }, "~/.claude", "", &mut o);
    ln(if util::skills_dir().is_dir() { "OK" } else { "WARN" }, "skills dir", "", &mut o);

    // settings.json válido.
    match settings::settings_valid() {
        Some(true) | None => ln("OK", "settings.json", "", &mut o),
        Some(false) => ln("FAIL", "settings.json", "inválido", &mut o),
    }

    // overdev hooks + agente (informativo).
    ln("OK", "overdev hooks", if settings::overdev_enabled() { "on" } else { "off" }, &mut o);
    ln("OK", "agente residente", if autostart::is_active() { "ativo" } else { "inativo" }, &mut o);

    // PATH shadow.
    match shadowed() {
        Some(p) => ln("WARN", "PATH", &format!("shadow: {p} sombreia /usr/bin/schematize"), &mut o),
        None => ln("OK", "PATH", "", &mut o),
    }

    // Rede (GitHub).
    ln(if github_reachable() { "OK" } else { "FAIL" }, "rede (GitHub)", "", &mut o);

    // Versão do CLI vs latest.
    let cur = env!("CARGO_PKG_VERSION");
    match skills::latest_version_raw("schematize-cli") {
        Some(l) if l != cur => ln("WARN", "versão CLI", &format!("{cur} → {l}"), &mut o),
        _ => ln("OK", "versão CLI", cur, &mut o),
    }

    // GUI flavor (só leitura).
    match find_gui_bin() {
        Some(path) => match fs::read(&path) {
            Ok(bytes) => {
                let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
                if has(b"slint") {
                    ln("OK", "GUI (schematize-gui)", &format!("Slint — {}", path.display()), &mut o);
                } else if has(b"eframe") || has(b"egui") {
                    ln("WARN", "GUI (schematize-gui)", "GUI antiga (egui) — rode `schematize upgrade`", &mut o);
                } else {
                    ln("WARN", "GUI (schematize-gui)", "não parece o Slint", &mut o);
                }
            }
            Err(e) => ln("WARN", "GUI (schematize-gui)", &format!("ilegível: {e}"), &mut o),
        },
        None => ln("OK", "GUI (schematize-gui)", "não instalado (usa a embutida)", &mut o),
    }

    // Lançador .desktop: Exec absoluto?
    let desktop = util::home().join(".local/share/applications/schematize-gui.desktop");
    if desktop.exists() {
        if let Ok(content) = fs::read_to_string(&desktop) {
            let exec = content.lines().find_map(|l| l.strip_prefix("Exec=")).map(str::trim).unwrap_or("");
            if exec.starts_with('/') {
                ln("OK", "Lançador .desktop", "Exec com caminho absoluto", &mut o);
            } else {
                ln("WARN", "Lançador .desktop", "Exec não-absoluto — `schematize doctor --fix`", &mut o);
            }
        }
    }

    o
}

/// Retorna o caminho do binário que sombreia /usr/bin/schematize, se houver.
fn shadowed() -> Option<String> {
    let bin = util::run("bash", &["-lc", "command -v schematize"]).ok()?;
    let bin = bin.trim().to_string();
    if !bin.is_empty() && bin != "/usr/bin/schematize" && Path::new("/usr/bin/schematize").exists() {
        Some(bin)
    } else {
        None
    }
}

/// GitHub acessível? (HEAD rápido na API, sem baixar corpo.)
fn github_reachable() -> bool {
    util::run("curl", &["-sfI", "-m", "6", "https://api.github.com"]).is_ok()
}

/// Localiza o binário `schematize-gui` no PATH. Prioriza o resolvido pelo shell
/// (`command -v`, que respeita a ordem do PATH); cai em ~/.cargo/bin e /usr/bin.
fn find_gui_bin() -> Option<std::path::PathBuf> {
    if let Ok(out) = util::run("bash", &["-lc", "command -v schematize-gui"]) {
        let p = out.trim();
        if !p.is_empty() {
            let pb = std::path::PathBuf::from(p);
            if pb.exists() {
                return Some(pb);
            }
        }
    }
    for c in [util::home().join(".cargo/bin/schematize-gui"), std::path::PathBuf::from("/usr/bin/schematize-gui")] {
        if c.exists() {
            return Some(c);
        }
    }
    None
}

/// Checagens da GUI. Devolve quantos problemas (WARN/FAIL) contabilizou.
/// - flavor: o binário instalado é o Slint (novo) ou o egui (antigo)?
/// - lançador: o `.desktop` usa `Exec=` com caminho ABSOLUTO? (com --fix, reescreve)
/// - coexistência: pacote (/usr/bin) + fonte (~/.cargo/bin) ao mesmo tempo (informativo).
fn gui_checks(fix: bool) -> usize {
    let mut issues = 0usize;

    // 1) Flavor: lê os bytes do binário e procura "slint" (novo) vs "eframe"/"egui" (antigo).
    let gui_bin = find_gui_bin();
    match &gui_bin {
        Some(path) => match fs::read(path) {
            Ok(bytes) => {
                let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
                let is_slint = has(b"slint");
                let is_egui = has(b"eframe") || has(b"egui");
                if is_slint {
                    line(&Lv::Ok, "GUI (schematize-gui)", &format!("Slint — {}", path.display()));
                } else if is_egui {
                    issues += 1;
                    line(
                        &Lv::Warn,
                        "GUI (schematize-gui)",
                        "é a GUI antiga (egui), não o Slint — rode `schematize upgrade` para reconstruir",
                    );
                } else {
                    issues += 1;
                    line(
                        &Lv::Warn,
                        "GUI (schematize-gui)",
                        "não parece o Slint — rode `schematize upgrade` para reconstruir",
                    );
                }
            }
            Err(e) => line(&Lv::Warn, "GUI (schematize-gui)", &format!("ilegível: {e}")),
        },
        None => line(&Lv::Ok, "GUI (schematize-gui)", "não instalado (usa a GUI embutida)"),
    }

    // 2) Lançador .desktop: o Exec= tem que ser um caminho ABSOLUTO (senão o ambiente
    //    gráfico, sem ~/.cargo/bin no PATH, abre o schematize-gui errado do /usr/bin).
    let desktop = util::home().join(".local/share/applications/schematize-gui.desktop");
    if desktop.exists() {
        match fs::read_to_string(&desktop) {
            Ok(content) => {
                let exec = content
                    .lines()
                    .find_map(|l| l.strip_prefix("Exec="))
                    .map(str::trim)
                    .unwrap_or("");
                let absolute = exec.starts_with('/');
                if absolute {
                    line(&Lv::Ok, "Lançador .desktop", "Exec com caminho absoluto");
                } else if fix {
                    match gui_bin.as_ref() {
                        Some(bin) => match write_desktop(&desktop, bin) {
                            Ok(()) => {
                                let _ = util::run(
                                    "update-desktop-database",
                                    &[desktop.parent().and_then(|p| p.to_str()).unwrap_or("")],
                                );
                                line(&Lv::Ok, "Lançador .desktop", &format!("corrigido → Exec={}", bin.display()));
                            }
                            Err(e) => {
                                issues += 1;
                                line(&Lv::Warn, "Lançador .desktop", &format!("falha ao corrigir: {e}"));
                            }
                        },
                        None => {
                            issues += 1;
                            line(&Lv::Warn, "Lançador .desktop", "Exec não-absoluto e schematize-gui não encontrado para corrigir");
                        }
                    }
                } else {
                    issues += 1;
                    line(
                        &Lv::Warn,
                        "Lançador .desktop",
                        "Exec não é caminho absoluto — rode `schematize doctor --fix` para reescrever",
                    );
                }
            }
            Err(e) => line(&Lv::Warn, "Lançador .desktop", &format!("ilegível: {e}")),
        }
    }

    // 3) Pacote + fonte coexistindo: informa que a de fonte (~/.cargo/bin, à frente no
    //    PATH) é a que vale e sugere remover o pacote se quiser limpar.
    let pkg = std::path::Path::new("/usr/bin/schematize-gui");
    let src = util::home().join(".cargo/bin/schematize-gui");
    if pkg.exists() && src.exists() {
        line(
            &Lv::Ok,
            "Instalações de GUI",
            "duas: pacote (/usr/bin) + fonte (~/.cargo/bin); a de fonte vale (PATH à frente) — `sudo apt remove schematize` (ou zypper) p/ limpar o pacote",
        );
    }

    issues
}

/// Reescreve o `.desktop` do schematize-gui com `Exec=<caminho absoluto>` (mesmo
/// formato que o install.sh usa: Type/Name/Exec/Terminal=false/Categories).
fn write_desktop(path: &Path, bin: &Path) -> Result<(), String> {
    // Gera o ícone em TODOS os locais de fallback e usa o 256px ABSOLUTO no Icon= (no Wayland o dock
    // depende do .desktop; caminho absoluto é à prova de cache/tema). Se falhar, cai pro nome "schematize".
    let icon = crate::appicon::install_all(&util::home())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "schematize".into());
    let body = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=schematize\n\
         GenericName=Ecossistema schematize\n\
         Comment=Skills, overdev e mais — schematize\n\
         Exec={}\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Development;Utility;\n\
         Keywords=schematize;skills;overdev;claude;\n\
         StartupWMClass=schematize-gui\n",
        bin.display()
    );
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(path, body).map_err(|e| e.to_string())?;
    // Atualiza os caches best-effort (o DE pode não ter as ferramentas).
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-t"])
        .arg(util::home().join(".local/share/icons/hicolor"))
        .status();
    let _ = std::process::Command::new("update-desktop-database")
        .arg(util::home().join(".local/share/applications"))
        .status();
    Ok(())
}
