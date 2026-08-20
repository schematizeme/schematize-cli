//! As SEÇÕES do relatório — cada uma coleta um recorte da máquina (sistema,
//! instalação, PATH, dependências, config, skills, overdev, updater, doctor, logs).

use super::*;

/// 1) Sistema: versões, OS, kernel, arch, desktop/session, shell.
pub(crate) fn sec_sistema(o: &mut String) {
    hdr(o, "1. SISTEMA");
    kv(o, "schematize (CLI)", env!("CARGO_PKG_VERSION"));

    // GUI: versão (best-effort) + flavor (bytes: slint novo vs eframe/egui antigo).
    match which_all("schematize-gui").into_iter().next() {
        Some(bin) => {
            let ver = cmd_out("schematize-gui", &["--version"]);
            kv(o, "schematize-gui (bin)", &bin.display().to_string());
            kv(o, "schematize-gui --version", &ver);
            kv(o, "schematize-gui flavor", &gui_flavor(&bin));
        }
        None => kv(o, "schematize-gui", "não encontrado no PATH"),
    }

    let (name, ver) = os_release();
    kv(o, "OS", &format!("{name} {ver}"));
    kv(o, "kernel (uname -a)", &cmd_out("uname", &["-a"]));
    kv(o, "arch", std::env::consts::ARCH);
    kv(o, "XDG_CURRENT_DESKTOP", &getenv("XDG_CURRENT_DESKTOP"));
    kv(o, "XDG_SESSION_TYPE", &getenv("XDG_SESSION_TYPE"));
    kv(o, "SHELL", &getenv("SHELL"));
}

/// 2) Instalação: binários no PATH (shadows), pacote, lançadores, autostart.
pub(crate) fn sec_instalacao(o: &mut String) {
    hdr(o, "2. INSTALAÇÃO");
    for name in ["schematize", "schematize-gui"] {
        let all = which_all(name);
        if all.is_empty() {
            kv(o, name, "não está no PATH");
        } else {
            let _ = writeln!(o, "  {name} no PATH ({}{}):", all.len(), if all.len() > 1 { " — SHADOW!" } else { "" });
            for p in &all {
                let _ = writeln!(o, "    - {}", describe_file(p));
            }
        }
    }

    // Coexistência de pacote (dpkg/rpm).
    kv(o, "dpkg -l schematize", &pkg_query_line(&cmd_out("dpkg-query", &["-W", "-f=${Version}", "schematize"])));
    kv(o, "rpm -q schematize", &pkg_query_line(&cmd_out("rpm", &["-q", "schematize"])));

    // Lançadores .desktop (linha Exec=).
    let home = util::home();
    for desktop in [
        home.join(".local/share/applications/schematize-gui.desktop"),
        PathBuf::from("/usr/share/applications/schematize-gui.desktop"),
    ] {
        if desktop.exists() {
            let exec = read_first_prefix(&desktop, "Exec=");
            kv(o, &format!("launcher {}", desktop.display()), &format!("Exec={}", exec));
        }
    }

    // Autostart (entradas schematize*).
    let _ = writeln!(o, "  autostart (schematize*):");
    let mut any_autostart = false;
    for dir in [home.join(".config/autostart"), PathBuf::from("/etc/xdg/autostart")] {
        for entry in list_dir_named(&dir, "schematize") {
            any_autostart = true;
            let _ = writeln!(o, "    - {}", entry.display());
        }
    }
    if !any_autostart {
        let _ = writeln!(o, "    (nenhuma)");
    }
}

/// 3) PATH completo + env selecionado (REDIGIDO pela camada final).
pub(crate) fn sec_path_env(o: &mut String) {
    hdr(o, "3. PATH + AMBIENTE");
    kv(o, "PATH", &getenv("PATH"));
    for name in ["HOME", "LANG", "LC_ALL", "TERM", "RUST_LOG", "SHELL"] {
        kv(o, name, &getenv(name));
    }
    // Todas as XDG_* (útil pro lançador GUI).
    let mut xdg: Vec<(String, String)> = std::env::vars().filter(|(k, _)| k.starts_with("XDG_")).collect();
    xdg.sort();
    for (k, v) in xdg {
        kv(o, &k, &v);
    }
}

/// 4) Dependências: claude, terminais, node/npm/cargo, libfontconfig.
pub(crate) fn sec_dependencias(o: &mut String) {
    hdr(o, "4. DEPENDÊNCIAS");
    kv(o, "claude no PATH?", if crate::agentrun::claude_in_path() { "sim" } else { "não" });
    // Resolve pelo mesmo caminho que o launch usa ($PATH + fallback ~/.local/bin etc.) pra o
    // relatório não divergir (ex.: "no PATH? sim" mas "--version indisponível" sob PATH mínimo da GUI).
    match crate::agentrun::claude_path() {
        Some(p) => {
            kv(o, "claude (path)", &p.display().to_string());
            kv(o, "claude --version", &cmd_out(&p.display().to_string(), &["--version"]));
        }
        None => {
            kv(o, "claude (path)", "—");
            kv(o, "claude --version", "(claude não encontrado)");
        }
    }

    let terms = [
        "konsole",
        "gnome-terminal",
        "xfce4-terminal",
        "x-terminal-emulator",
        "xterm",
        "alacritty",
        "kitty",
    ];
    let present: Vec<&str> = terms.iter().copied().filter(|t| has_bin(t)).collect();
    kv(o, "terminais disponíveis", &if present.is_empty() { "(nenhum)".into() } else { present.join(", ") });

    kv(o, "node --version", &cmd_out("node", &["--version"]));
    kv(o, "npm --version", &cmd_out("npm", &["--version"]));
    kv(o, "cargo --version", &cmd_out("cargo", &["--version"]));
    kv(o, "libfontconfig (GUI Slint)", &fontconfig_present());
}

/// 5) Config (REDIGIDO): NUNCA lê auth.json/ssh/API keys — só nomes+tamanhos e flags.
pub(crate) fn sec_config(o: &mut String) {
    hdr(o, "5. CONFIG (redigido — sem conteúdo de auth.json/ssh)");

    // Listagem de ~/.schematize/ — SÓ nome + tamanho (nunca o conteúdo).
    let sdir = util::home_app_dir();
    let _ = writeln!(o, "  ~/.schematize/ (nome + tamanho — conteúdo NÃO lido):");
    match fs::read_dir(&sdir) {
        Ok(rd) => {
            let mut ents: Vec<(String, u64, bool)> = Vec::new();
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                let (size, is_dir) = match fs::metadata(&p) {
                    Ok(md) => (md.len(), md.is_dir()),
                    Err(_) => (0, false),
                };
                ents.push((name, size, is_dir));
            }
            ents.sort();
            if ents.is_empty() {
                let _ = writeln!(o, "    (vazio)");
            }
            for (name, size, is_dir) in ents {
                let _ = writeln!(o, "    - {name}{}  ({size} bytes)", if is_dir { "/" } else { "" });
            }
        }
        Err(e) => {
            let _ = writeln!(o, "    (indisponível: {e})");
        }
    }

    let cfg = config::load();
    kv(o, "lang", cfg.lang.as_deref().unwrap_or("(auto)"));
    kv(o, "dev_dirs", &list_or_empty(&config::dev_dirs()));
    kv(o, "projects (pin)", &list_or_empty(&config::projects()));
    kv(o, "recent_projects", &list_or_empty(&config::recent_projects()));
    kv(o, "logado?", if account::is_logged_in() { "sim" } else { "não" });
    kv(o, "account sub", &account::account_sub().unwrap_or_else(|| "(não logado)".into()));
    let _ = writeln!(o, "  (token de sessão e chaves privadas NÃO são lidos por desenho.)");
}

/// 6) Skills: instaladas + versões + forkadas; alcance do catálogo.
pub(crate) fn sec_skills(o: &mut String, online: bool) {
    hdr(o, "6. SKILLS");
    let st = skills::load_state();
    if st.skills.is_empty() {
        let _ = writeln!(o, "  (nenhuma skill registrada no state)");
    }
    for (slug, e) in &st.skills {
        let fork = if e.forked {
            format!(" [FORK, base v{}]", e.fork_base_version.as_deref().unwrap_or("?"))
        } else {
            String::new()
        };
        let _ = writeln!(o, "  {slug:<16} v{}{fork}", e.version);
    }
    if online {
        let cat = registry::catalog();
        kv(o, "catálogo (alcance)", &format!("{} skills{}", cat.len(), if cat.len() >= 19 { " (remoto ok)" } else { " (embutido? offline?)" }));
    } else {
        kv(o, "catálogo (alcance)", "(pulado — offline)");
    }
}

/// 7) Overdev: varre dev_dirs+recent_projects por `.overdev/state.json`. Devolve as raízes achadas.
pub(crate) fn sec_overdev(o: &mut String) -> Vec<PathBuf> {
    hdr(o, "7. OVERDEV");
    let roots = overdev_roots();
    if roots.is_empty() {
        let _ = writeln!(o, "  (nenhum projeto com .overdev/state.json nas dev_dirs/recent_projects)");
        return roots;
    }
    for root in &roots {
        let prog = overdev::progress_at(root);
        let checklist = crate::paths::overdev_dir_at(root).join("CHECKLIST.md");
        let mt = fs::metadata(&checklist)
            .ok()
            .and_then(|md| md.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| fmt_epoch(d.as_secs()))
            .unwrap_or_else(|| "?".into());
        let _ = writeln!(o, "  {}", root.display());
        let _ = writeln!(
            o,
            "    mode={} feitos={} abertos={} on-hold={} humanos={} iters={}/{} · CHECKLIST mtime={}",
            prog.mode, prog.done, prog.open, prog.hold, prog.human, prog.iterations, prog.max_iters, mt
        );
    }
    roots
}

/// 8) Updater: reusa o diagnóstico textual do módulo `debug`.
pub(crate) fn sec_updater(o: &mut String) {
    hdr(o, "8. UPDATER (versionamento/self-update)");
    let _ = writeln!(o, "{}", indent(&debug::report_text(), "  "));
}

/// 9) Doctor: reusa os checks read-only do módulo `doctor`.
pub(crate) fn sec_doctor(o: &mut String) {
    hdr(o, "9. DOCTOR (checks read-only)");
    let _ = writeln!(o, "{}", indent(&doctor::report_text(), "  "));
}

/// 10) Logs: tail do update.log e de premature-stops.log de cada overdev achado.
pub(crate) fn sec_logs(o: &mut String, overdev_roots: &[PathBuf]) {
    hdr(o, "10. LOGS");
    let update_log = util::dados_dir().join("update.log");
    let _ = writeln!(o, "  {} (últimas 40 linhas):", update_log.display());
    let _ = writeln!(o, "{}", indent(&tail(&update_log, 40), "    "));

    for root in overdev_roots {
        let plog = crate::paths::overdev_dir_at(root).join("premature-stops.log");
        if plog.exists() {
            let _ = writeln!(o, "  {} (últimas 40 linhas):", plog.display());
            let _ = writeln!(o, "{}", indent(&tail(&plog, 40), "    "));
        }
    }
}
