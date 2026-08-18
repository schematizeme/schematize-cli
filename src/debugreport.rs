//! `schematize debug --collect` — COLETOR DE DEBUG.
//! O quê: junta num único relatório de texto TUDO que ajuda a diagnosticar a
//! ferramenta na máquina de outro usuário (sistema, instalação, PATH, dependências,
//! config, skills, overdev, updater, doctor, logs) pra ele compartilhar.
//! Onde: `schematize debug --collect [--out <path>] [--stdout]`.
//!
//! PRIORIDADE Nº1 — NUNCA VAZAR SEGREDO. Duas camadas de defesa:
//!  1) EVITAÇÃO: NUNCA lê o conteúdo de `~/.schematize/auth.json` (token de sessão),
//!     de `~/.ssh/*` (chaves privadas), nem de qualquer arquivo de chave de API. O
//!     `~/.schematize/` é listado só por NOME+TAMANHO; a sessão é reportada como
//!     "logado sim/não" + o `sub` (id interno, não é segredo).
//!  2) REDAÇÃO: `scrub()` é aplicada ao relatório INTEIRO no fim — qualquer token
//!     (re_/sk-/ghp_/xox…/JWT/Bearer), bloco de chave privada, ou par
//!     `KEY=/TOKEN=/SECRET=/PASS…=` (e toda var de ambiente cujo NOME contenha
//!     KEY/TOKEN/SECRET/PASS/CRED) vira `<REDIGIDO>` antes de sair.
//!
//! Postura: TUDO best-effort — nunca panica; a falha de uma seção vira
//! "(indisponível: <motivo>)" e o resto do relatório segue.

use crate::{account, config, debug, doctor, overdev, registry, skills, util};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// ================================================================================================
// REDAÇÃO (scrub) — a rede de segurança. Sem crate de regex: varredura à mão (estilo da casa).
// ================================================================================================

/// Placeholder que substitui qualquer segredo detectado.
const RED: &str = "<REDIGIDO>";

/// Redige segredos de um texto qualquer vindo de env/arquivos/comandos. Cobre:
/// - tokens por prefixo: `re_…`, `sk-…`, `ghp_/gho_/ghu_/ghs_/ghr_/github_pat_…`, `xox[bap]-…`, `xapp-…`
/// - JWTs `eyJ….….…` (3 partes base64url)
/// - `Bearer <token>` (o token vira `<REDIGIDO>`)
/// - blocos `-----BEGIN … PRIVATE KEY-----` … `-----END …-----` (o bloco todo some)
/// - `NOME=valor` quando NOME contém KEY/TOKEN/SECRET/PASS/CRED, OU o valor parece um token
/// Idempotente e best-effort — na dúvida, redige (segurança-primeiro).
pub fn scrub(s: &str) -> String {
    // 1) Blocos de chave privada primeiro (são multi-linha).
    let s = redact_private_key_blocks(s);
    // 2) Linha a linha, palavra a palavra.
    let mut out: Vec<String> = Vec::new();
    for line in s.lines() {
        out.push(scrub_line(line));
    }
    out.join("\n")
}

/// Some com o miolo de qualquer bloco PEM de chave PRIVADA (defesa extra — nós nunca
/// lemos ~/.ssh, mas se algum comando cuspir um bloco, ele não passa).
fn redact_private_key_blocks(s: &str) -> String {
    if !s.contains("PRIVATE KEY-----") {
        return s.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    let mut in_key = false;
    for l in s.lines() {
        if !in_key && l.contains("-----BEGIN") && l.contains("PRIVATE KEY-----") {
            in_key = true;
            out.push(format!("{RED} (bloco de chave privada)"));
            continue;
        }
        if in_key {
            if l.contains("-----END") {
                in_key = false;
            }
            continue; // descarta as linhas do bloco
        }
        out.push(l.to_string());
    }
    out.join("\n")
}

/// Redige uma única linha: quebra em segmentos (espaço vs palavra), preserva o
/// espaçamento e aplica `scrub_word` em cada palavra. Trata `Bearer <token>`.
fn scrub_line(line: &str) -> String {
    let mut result = String::new();
    let mut prev_bearer = false;
    for (is_ws, seg) in segments(line) {
        if is_ws {
            result.push_str(&seg);
            continue;
        }
        if prev_bearer {
            result.push_str(RED);
            prev_bearer = false;
            continue;
        }
        prev_bearer = seg.eq_ignore_ascii_case("bearer");
        result.push_str(&scrub_word(&seg));
    }
    result
}

/// Quebra a linha em segmentos alternados (whitespace, não-whitespace), preservando tudo.
fn segments(line: &str) -> Vec<(bool, String)> {
    let mut segs: Vec<(bool, String)> = Vec::new();
    let mut cur = String::new();
    let mut cur_ws: Option<bool> = None;
    for ch in line.chars() {
        let ws = ch.is_whitespace();
        match cur_ws {
            Some(p) if p == ws => cur.push(ch),
            Some(p) => {
                segs.push((p, std::mem::take(&mut cur)));
                cur.push(ch);
                cur_ws = Some(ws);
            }
            None => {
                cur.push(ch);
                cur_ws = Some(ws);
            }
        }
    }
    if let Some(p) = cur_ws {
        segs.push((p, cur));
    }
    segs
}

/// Redige UMA palavra (sem espaços): pares `NOME=valor` sensíveis e tokens soltos.
fn scrub_word(word: &str) -> String {
    // Par NOME=valor: redige o valor se o NOME é sensível OU o valor parece um token.
    if let Some(eq) = word.find('=') {
        let left = &word[..eq];
        let right = &word[eq + 1..];
        if !right.is_empty() && (key_is_secret(left) || looks_like_secret_token(right)) {
            return format!("{left}={RED}");
        }
    }
    // Token solto (com ou sem pontuação ao redor).
    if looks_like_secret_token(word) {
        return RED.to_string();
    }
    word.to_string()
}

/// O NOME (lado esquerdo do `=`) indica segredo? Substring case-insensitive dos gatilhos.
fn key_is_secret(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASS", "CRED"].iter().any(|k| up.contains(k))
}

/// A palavra CONTÉM algo que parece um token/segredo (prefixo conhecido + charset suficiente, ou JWT)?
fn looks_like_secret_token(w: &str) -> bool {
    // JWT em qualquer posição.
    if let Some(idx) = w.find("eyJ") {
        if is_jwt(&w[idx..]) {
            return true;
        }
    }
    // Prefixos conhecidos, seguidos de >=8 chars do charset do token.
    const PREFIXES: &[&str] = &[
        "re_", "sk-", "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_", "xoxb-", "xoxa-",
        "xoxp-", "xapp-",
    ];
    for pfx in PREFIXES {
        if let Some(idx) = w.find(pfx) {
            let after = &w[idx + pfx.len()..];
            let n = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count();
            if n >= 8 {
                return true;
            }
        }
    }
    false
}

/// A fatia começa com um JWT `eyJ…`.`…`.`…` (3 partes base64url não-vazias)?
fn is_jwt(s: &str) -> bool {
    let run: String = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .collect();
    let parts: Vec<&str> = run.split('.').collect();
    if parts.len() < 3 || !parts[0].starts_with("eyJ") {
        return false;
    }
    parts
        .iter()
        .take(3)
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
}

// ================================================================================================
// COLETA — monta o relatório seção a seção. Tudo best-effort.
// ================================================================================================

/// Monta o relatório COMPLETO (texto). Aplica `scrub` no fim, como rede de segurança
/// sobre tudo que entrou de env/arquivo/comando.
/// Monta o relatório. `online=false` (default) é OFFLINE-first e RÁPIDO — pula as seções que
/// batem na rede (updater/rate-limit do GitHub, alcance do catálogo, doctor/github_reachable),
/// que podem TRAVAR numa máquina com rede bloqueada/lenta (curl sem timeout curto). Com
/// `online=true` inclui esses diagnósticos de rede (úteis pra bug de versionamento).
pub fn collect(online: bool) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "===== SCHEMATIZE DEBUG REPORT =====");
    let _ = writeln!(o, "gerado em: {} (epoch {})", fmt_epoch(util::now_unix()), util::now_unix());
    let _ = writeln!(o, "modo: {}", if online { "online (inclui rede)" } else { "offline (rápido; use --online p/ rede)" });
    let _ = writeln!(o, "AVISO: segredos são redigidos automaticamente; revise antes de compartilhar.");

    sec_sistema(&mut o);
    sec_instalacao(&mut o);
    sec_path_env(&mut o);
    sec_dependencias(&mut o);
    sec_config(&mut o);
    sec_skills(&mut o, online);
    let overdev_roots = sec_overdev(&mut o);
    if online {
        sec_updater(&mut o);
        sec_doctor(&mut o);
    } else {
        hdr(&mut o, "8-9. UPDATER + DOCTOR (rede)");
        let _ = writeln!(&mut o, "  (pulados no modo offline — rode `schematize debug --collect --online` p/ incluir)");
    }
    sec_logs(&mut o, &overdev_roots);

    let _ = writeln!(o, "\n===== FIM =====");

    // Rede de segurança final: redige o relatório inteiro.
    scrub(&o)
}

/// Grava o relatório em `out` (ou `~/.schematize/debug-report-<epoch>.txt`), modo 600.
/// Retorna o caminho gravado. Cria `~/.schematize` (modo 700) se preciso.
pub fn write_report(out: Option<&Path>, online: bool) -> Result<PathBuf, String> {
    let report = collect(online);
    let path = match out {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = util::home().join(".schematize");
            fs::create_dir_all(&dir).map_err(|e| format!("falha ao criar {}: {e}", dir.display()))?;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
            dir.join(format!("debug-report-{}.txt", util::now_unix()))
        }
    };
    fs::write(&path, report.as_bytes())
        .map_err(|e| format!("falha ao gravar {}: {e}", path.display()))?;
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    Ok(path)
}

/// Resumo curto (pro CLI imprimir depois de gravar / pra GUI). Sem segredo.
pub fn short_summary() -> String {
    let logged = if account::is_logged_in() { "sim" } else { "não" };
    let n_skills = skills::load_state().skills.len();
    format!(
        "schematize v{} · logado: {logged} · skills instaladas: {n_skills}",
        env!("CARGO_PKG_VERSION")
    )
}

// ------------------------------------------------------------------------------------------------
// Seções.
// ------------------------------------------------------------------------------------------------

/// 1) Sistema: versões, OS, kernel, arch, desktop/session, shell.
fn sec_sistema(o: &mut String) {
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
fn sec_instalacao(o: &mut String) {
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
fn sec_path_env(o: &mut String) {
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
fn sec_dependencias(o: &mut String) {
    hdr(o, "4. DEPENDÊNCIAS");
    kv(o, "claude no PATH?", if crate::agentrun::claude_in_path() { "sim" } else { "não" });
    match which_all("claude").into_iter().next() {
        Some(p) => kv(o, "claude (path)", &p.display().to_string()),
        None => kv(o, "claude (path)", "—"),
    }
    kv(o, "claude --version", &cmd_out("claude", &["--version"]));

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
fn sec_config(o: &mut String) {
    hdr(o, "5. CONFIG (redigido — sem conteúdo de auth.json/ssh)");

    // Listagem de ~/.schematize/ — SÓ nome + tamanho (nunca o conteúdo).
    let sdir = util::home().join(".schematize");
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
fn sec_skills(o: &mut String, online: bool) {
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
fn sec_overdev(o: &mut String) -> Vec<PathBuf> {
    hdr(o, "7. OVERDEV");
    let roots = overdev_roots();
    if roots.is_empty() {
        let _ = writeln!(o, "  (nenhum projeto com .overdev/state.json nas dev_dirs/recent_projects)");
        return roots;
    }
    for root in &roots {
        let prog = overdev::progress_at(root);
        let checklist = root.join(".overdev").join("CHECKLIST.md");
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
fn sec_updater(o: &mut String) {
    hdr(o, "8. UPDATER (versionamento/self-update)");
    let _ = writeln!(o, "{}", indent(&debug::report_text(), "  "));
}

/// 9) Doctor: reusa os checks read-only do módulo `doctor`.
fn sec_doctor(o: &mut String) {
    hdr(o, "9. DOCTOR (checks read-only)");
    let _ = writeln!(o, "{}", indent(&doctor::report_text(), "  "));
}

/// 10) Logs: tail do update.log e de premature-stops.log de cada overdev achado.
fn sec_logs(o: &mut String, overdev_roots: &[PathBuf]) {
    hdr(o, "10. LOGS");
    let update_log = util::claude_dir().join("schematize").join("update.log");
    let _ = writeln!(o, "  {} (últimas 40 linhas):", update_log.display());
    let _ = writeln!(o, "{}", indent(&tail(&update_log, 40), "    "));

    for root in overdev_roots {
        let plog = root.join(".overdev").join("premature-stops.log");
        if plog.exists() {
            let _ = writeln!(o, "  {} (últimas 40 linhas):", plog.display());
            let _ = writeln!(o, "{}", indent(&tail(&plog, 40), "    "));
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Helpers de coleta/formatação.
// ------------------------------------------------------------------------------------------------

/// Cabeçalho de seção (linha em branco antes).
fn hdr(o: &mut String, title: &str) {
    let _ = writeln!(o, "\n== {title} ==");
}

/// Linha chave/valor alinhada.
fn kv(o: &mut String, k: &str, v: &str) {
    let _ = writeln!(o, "  {k:<26} {v}");
}

/// Valor de uma env var (ou marcador se ausente).
fn getenv(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| "(não definido)".to_string())
}

/// Roda um comando capturando stdout, LIMITADO a 5s via `timeout` — a máquina de outro
/// usuário pode ter binário que trava (ex.: `schematize-gui --version` que abre a janela
/// em vez de sair). Erro/ausência/estouro vira "(indisponível: …)". Best-effort.
fn cmd_out(bin: &str, args: &[&str]) -> String {
    let mut full: Vec<&str> = vec!["5", bin];
    full.extend_from_slice(args);
    match util::run("timeout", &full) {
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() {
                "(vazio)".to_string()
            } else {
                // Só a 1ª linha pra saídas multi-linha de --version.
                t.lines().next().unwrap_or("").to_string()
            }
        }
        // Colapsa erro multi-linha (ex.: stderr do gui) na 1ª linha, pra não quebrar o layout.
        Err(e) => format!("(indisponível: {})", e.lines().next().unwrap_or("").trim()),
    }
}

/// Normaliza a saída de dpkg-query/rpm num status legível.
fn pkg_query_line(s: &str) -> String {
    if s.starts_with("(indisponível") {
        "não instalado por pacote (ou gerenciador ausente)".to_string()
    } else {
        format!("instalado por pacote: {s}")
    }
}

/// Todos os caminhos de `name` encontrados na ordem do `$PATH` (dedup). >1 = shadow.
fn which_all(name: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(name);
            if p.is_file() && !found.contains(&p) {
                found.push(p);
            }
        }
    }
    found
}

/// `name` está em algum diretório do `$PATH`?
fn has_bin(name: &str) -> bool {
    !which_all(name).is_empty()
}

/// Descreve um arquivo: caminho + tamanho + mtime.
fn describe_file(p: &Path) -> String {
    match fs::metadata(p) {
        Ok(md) => {
            let mt = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| fmt_epoch(d.as_secs()))
                .unwrap_or_else(|| "?".into());
            format!("{} ({} bytes, mtime {})", p.display(), md.len(), mt)
        }
        Err(e) => format!("{} (metadata indisponível: {e})", p.display()),
    }
}

/// Flavor da GUI: lê os bytes e procura "slint" (novo) vs "eframe"/"egui" (antigo).
fn gui_flavor(bin: &Path) -> String {
    match fs::read(bin) {
        Ok(bytes) => {
            let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
            if has(b"slint") {
                "Slint (novo)".to_string()
            } else if has(b"eframe") || has(b"egui") {
                "egui/eframe (ANTIGO — rode `schematize upgrade`)".to_string()
            } else {
                "desconhecido (nem slint nem egui detectado)".to_string()
            }
        }
        Err(e) => format!("(ilegível: {e})"),
    }
}

/// /etc/os-release → (NAME, VERSION|VERSION_ID). Best-effort.
fn os_release() -> (String, String) {
    let body = match fs::read_to_string("/etc/os-release") {
        Ok(s) => s,
        Err(e) => return (format!("(indisponível: {e})"), String::new()),
    };
    let get = |key: &str| -> Option<String> {
        body.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().trim_matches('"').to_string())
    };
    let name = get("NAME=").unwrap_or_else(|| "?".into());
    let ver = get("VERSION=").or_else(|| get("VERSION_ID=")).unwrap_or_else(|| "?".into());
    (name, ver)
}

/// libfontconfig presente? (ldconfig -p, ou caminhos comuns.)
fn fontconfig_present() -> String {
    if let Ok(out) = util::run("bash", &["-lc", "ldconfig -p 2>/dev/null | grep -i libfontconfig || true"]) {
        let t = out.trim();
        if !t.is_empty() {
            return format!("presente ({})", t.lines().next().unwrap_or("").trim());
        }
    }
    for cand in [
        "/usr/lib/x86_64-linux-gnu/libfontconfig.so.1",
        "/usr/lib64/libfontconfig.so.1",
        "/usr/lib/libfontconfig.so.1",
    ] {
        if Path::new(cand).exists() {
            return format!("presente ({cand})");
        }
    }
    "NÃO encontrada (a GUI Slint pode não abrir)".to_string()
}

/// Primeira linha de `path` que começa com `prefix`, sem o prefixo. "(ausente)" se nada.
fn read_first_prefix(path: &Path, prefix: &str) -> String {
    match fs::read_to_string(path) {
        Ok(s) => s
            .lines()
            .find_map(|l| l.strip_prefix(prefix))
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| "(sem linha Exec=)".into()),
        Err(e) => format!("(ilegível: {e})"),
    }
}

/// Entradas de `dir` cujo nome começa com `prefix`.
fn list_dir_named(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(prefix) {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

/// Renderiza uma lista curta (ou "(nenhum)").
fn list_or_empty(v: &[String]) -> String {
    if v.is_empty() {
        "(nenhum)".to_string()
    } else {
        v.join(", ")
    }
}

/// Raízes de overdev: dev_dirs + subdirs imediatos das dev_dirs + recent_projects que
/// tenham `.overdev/state.json`. Dedup e ordenado.
fn overdev_roots() -> Vec<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    for d in config::dev_dirs() {
        let p = PathBuf::from(&d);
        cands.push(p.clone());
        // um nível abaixo (dev_dir guarda-chuva).
        if let Ok(rd) = fs::read_dir(&p) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    cands.push(e.path());
                }
            }
        }
    }
    for r in config::recent_projects() {
        cands.push(PathBuf::from(r));
    }
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out: Vec<PathBuf> = Vec::new();
    for c in cands {
        if c.join(".overdev").join("state.json").is_file() && seen.insert(c.clone()) {
            out.push(c);
        }
    }
    out.sort();
    out
}

/// Últimas `n` linhas de um arquivo de texto (ou marcador).
fn tail(path: &Path, n: usize) -> String {
    match fs::read_to_string(path) {
        Ok(s) => {
            let lines: Vec<&str> = s.lines().collect();
            let start = lines.len().saturating_sub(n);
            let slice = &lines[start..];
            if slice.is_empty() {
                "(vazio)".to_string()
            } else {
                slice.join("\n")
            }
        }
        Err(_) => "(ausente)".to_string(),
    }
}

/// Indenta cada linha de `s` com `pad`.
fn indent(s: &str, pad: &str) -> String {
    s.lines().map(|l| format!("{pad}{l}")).collect::<Vec<_>>().join("\n")
}

/// Epoch (s) → `YYYY-MM-DD HH:MM:SS UTC` (algoritmo civil de Howard Hinnant, sem crate de data).
fn fmt_epoch(secs: u64) -> String {
    let secs = secs as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

// ================================================================================================
// TESTES — foco na scrub (o piso de segurança). Não tocam em rede/HOME.
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_redige_token_por_prefixo() {
        assert_eq!(scrub("re_abc12345XYZ"), RED);
        assert_eq!(scrub("sk-abcdefgh12345"), RED);
        assert_eq!(scrub("ghp_0123456789abcdef"), RED);
        assert_eq!(scrub("xoxb-1234567890-abcdef"), RED);
        // Curto demais depois do prefixo NÃO é tratado como token.
        assert_eq!(scrub("re_abc"), "re_abc");
    }

    #[test]
    fn scrub_redige_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert_eq!(scrub(jwt), RED);
        // JWT embutido numa frase: some, o resto fica.
        let out = scrub(&format!("token: {jwt} fim"));
        assert!(out.contains("token:"));
        assert!(out.contains("fim"));
        assert!(!out.contains("eyJ"));
        assert!(out.contains(RED));
    }

    #[test]
    fn scrub_redige_bearer() {
        let out = scrub("Authorization: Bearer abc123def456ghi");
        assert!(out.contains("Bearer <REDIGIDO>"), "got: {out}");
        assert!(!out.contains("abc123def456ghi"));
    }

    #[test]
    fn scrub_redige_key_value() {
        assert_eq!(scrub("API_KEY=supersecretvalue"), "API_KEY=<REDIGIDO>");
        assert_eq!(scrub("MY_TOKEN=abc"), "MY_TOKEN=<REDIGIDO>");
        assert_eq!(scrub("DB_PASSWORD=hunter2"), "DB_PASSWORD=<REDIGIDO>");
        assert_eq!(scrub("AWS_SECRET_ACCESS_KEY=zzz"), "AWS_SECRET_ACCESS_KEY=<REDIGIDO>");
        assert_eq!(scrub("MY_CRED=x"), "MY_CRED=<REDIGIDO>");
        // Nome com sensível preserva o NOME, redige só o valor.
        let out = scrub("export GITHUB_TOKEN=ghp_realtokenvalue1234");
        assert!(out.contains("GITHUB_TOKEN=<REDIGIDO>"), "got: {out}");
        assert!(!out.contains("ghp_realtokenvalue1234"));
    }

    #[test]
    fn scrub_preserva_texto_normal() {
        assert_eq!(scrub("hello world"), "hello world");
        assert_eq!(scrub("TERM=xterm-256color"), "TERM=xterm-256color");
        assert_eq!(scrub("LANG=en_US.UTF-8"), "LANG=en_US.UTF-8");
        assert_eq!(scrub("versao 0.30.0 instalada ok"), "versao 0.30.0 instalada ok");
        assert_eq!(scrub("/home/user/.cargo/bin/schematize"), "/home/user/.cargo/bin/schematize");
        // Preserva espaçamento e múltiplas linhas.
        assert_eq!(scrub("a  b\nc"), "a  b\nc");
    }

    #[test]
    fn scrub_redige_bloco_de_chave_privada() {
        let pem = "antes\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAAB3Nz...\nsecretline\n-----END OPENSSH PRIVATE KEY-----\ndepois";
        let out = scrub(pem);
        assert!(out.contains("antes"));
        assert!(out.contains("depois"));
        assert!(!out.contains("secretline"));
        assert!(!out.contains("AAAAB3Nz"));
        assert!(out.contains(RED));
    }

    #[test]
    fn scrub_valor_com_token_mesmo_sem_nome_sensivel() {
        // Nome NÃO sensível, mas o valor parece token → redige o valor.
        let out = scrub("foo=ghp_abcdefgh12345678");
        assert_eq!(out, "foo=<REDIGIDO>");
    }

    #[test]
    fn short_summary_nao_panica() {
        // Só sanidade: monta a string sem tocar em segredo.
        let s = short_summary();
        assert!(s.contains("schematize v"));
    }

    #[test]
    fn fmt_epoch_conhecido() {
        // 2021-01-01 00:00:00 UTC = 1609459200.
        assert_eq!(fmt_epoch(1_609_459_200), "2021-01-01 00:00:00 UTC");
    }
}
