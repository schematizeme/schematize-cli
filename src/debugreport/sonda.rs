//! Sondas do ambiente: quais binários existem, o que um arquivo é, distro, fontes,
//! e as listagens auxiliares. Nada aqui decide nada — só observa.

use super::*;

/// Valor de uma env var (ou marcador se ausente).
pub(crate) fn getenv(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| "(não definido)".to_string())
}

/// Roda um comando capturando stdout, LIMITADO a 5s via `timeout` — a máquina de outro
/// usuário pode ter binário que trava (ex.: `schematize-gui --version` que abre a janela
/// em vez de sair). Erro/ausência/estouro vira "(indisponível: …)". Best-effort.
pub(crate) fn cmd_out(bin: &str, args: &[&str]) -> String {
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
pub(crate) fn pkg_query_line(s: &str) -> String {
    if s.starts_with("(indisponível") {
        "não instalado por pacote (ou gerenciador ausente)".to_string()
    } else {
        format!("instalado por pacote: {s}")
    }
}

/// Todos os caminhos de `name` encontrados na ordem do `$PATH` (dedup). >1 = shadow.
pub(crate) fn which_all(name: &str) -> Vec<PathBuf> {
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
pub(crate) fn has_bin(name: &str) -> bool {
    !which_all(name).is_empty()
}

/// Descreve um arquivo: caminho + tamanho + mtime.
pub(crate) fn describe_file(p: &Path) -> String {
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
pub(crate) fn gui_flavor(bin: &Path) -> String {
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
pub(crate) fn os_release() -> (String, String) {
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
pub(crate) fn fontconfig_present() -> String {
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
pub(crate) fn read_first_prefix(path: &Path, prefix: &str) -> String {
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
pub(crate) fn list_dir_named(dir: &Path, prefix: &str) -> Vec<PathBuf> {
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
pub(crate) fn list_or_empty(v: &[String]) -> String {
    if v.is_empty() {
        "(nenhum)".to_string()
    } else {
        v.join(", ")
    }
}

/// Raízes de overdev: dev_dirs + subdirs imediatos das dev_dirs + recent_projects que
/// tenham `.overdev/state.json`. Dedup e ordenado.
pub(crate) fn overdev_roots() -> Vec<PathBuf> {
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
        if crate::paths::overdev_dir_at(&c).join("state.json").is_file() && seen.insert(c.clone()) {
            out.push(c);
        }
    }
    out.sort();
    out
}

/// Últimas `n` linhas de um arquivo de texto (ou marcador).
pub(crate) fn tail(path: &Path, n: usize) -> String {
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
