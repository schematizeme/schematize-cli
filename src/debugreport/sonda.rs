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
    if let Ok(out) =
        util::run("bash", &["-lc", "ldconfig -p 2>/dev/null | grep -i libfontconfig || true"])
    {
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

/// Primeiro valor de um arquivo `chave: valor` do /proc (ex.: `model name` do cpuinfo).
///
/// O quê: varre as linhas, casa a chave antes do `:` e devolve o valor aparado.
/// Onde: [`cpu_modelo`] e [`ram_total_mb`], na seção de HARDWARE do relatório.
/// **Entrada:** `path` do arquivo, `chave` exata (sem `:`).
/// **Saída:** o valor, ou `"(indisponível)"` se o arquivo/chave não existir.
/// **Efeitos:** lê o disco; nunca panica.
pub(crate) fn proc_campo(path: &str, chave: &str) -> String {
    let Ok(txt) = fs::read_to_string(path) else {
        return "(indisponível)".into();
    };
    txt.lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == chave).then(|| v.trim().to_string())
        })
        .unwrap_or_else(|| "(indisponível)".into())
}

/// Modelo do processador. Onde: seção HARDWARE (perf de build e do overdev dependem disto).
pub(crate) fn cpu_modelo() -> String {
    proc_campo("/proc/cpuinfo", "model name")
}

/// RAM total em MB, formatada. Onde: seção HARDWARE — build do fonte e nº de agentes
/// paralelos são limitados por ela, então é o 1º número a olhar num relato de "travou".
/// **Saída:** ex. `"15943 MB"`, ou `"(indisponível)"`.
pub(crate) fn ram_total_mb() -> String {
    match ram_total_mb_num() {
        Some(mb) => format!("{mb} MB"),
        None => "(indisponível)".into(),
    }
}

/// RAM total em MB como NÚMERO. Onde: o `env` do `POST /diagnostics`.
/// Por que separado da versão formatada: no `env` o campo é filtrável (JSONB), e
/// `"31478 MB"` como string impede a única pergunta que importa ali — "quais máquinas
/// têm menos de X?". Comparação numérica exige número. **Saída:** `None` se o /proc não deu.
pub(crate) fn ram_total_mb_num() -> Option<u64> {
    proc_campo("/proc/meminfo", "MemTotal")
        .split_whitespace()
        .next()
        .and_then(|n| n.parse::<u64>().ok())
        .map(|kb| kb / 1024)
}

/// Modelo da GPU, sem o slot PCI nem o `(rev xx)`.
///
/// O quê: de `03:00.0 VGA compatible controller: AMD [ATI] Lucienne (rev c1)` extrai
/// `AMD [ATI] Lucienne`. Onde: o `env` filtrável. Por que: agrupar por MODELO é a pergunta
/// de triagem ("os relatos são todos da mesma GPU?"); o slot muda por máquina e só quebra o
/// agrupamento. A linha CRUA continua na seção de texto do relatório.
pub(crate) fn gpu_modelo() -> String {
    let bruto = gpu_info();
    let primeira = bruto.split(" | ").next().unwrap_or(&bruto).to_string();
    let depois = primeira.split_once(": ").map(|(_, d)| d.to_string()).unwrap_or(primeira);
    match depois.rfind(" (rev ") {
        Some(i) => depois[..i].trim().to_string(),
        None => depois.trim().to_string(),
    }
}

/// Saída COMPLETA de um comando (todas as linhas), com timeout de 5s.
///
/// O quê: como [`cmd_out`], mas SEM cortar na 1ª linha. Onde: [`gpu_info`].
/// Por que existe: `cmd_out` é feito pra `--version` e devolve só a 1ª linha — usá-lo com
/// `lspci` fazia o filtro de VGA olhar apenas o `Host bridge` e concluir "nenhum adaptador",
/// numa máquina com Radeon na lista. Saída multi-linha precisa de leitor multi-linha.
/// **Entrada:** binário + args. **Saída:** stdout aparado, ou `"(indisponível: …)"`.
/// **Efeitos:** executa processo externo; nunca panica.
pub(crate) fn cmd_out_full(bin: &str, args: &[&str]) -> String {
    let mut full: Vec<&str> = vec!["5", bin];
    full.extend_from_slice(args);
    match util::run("timeout", &full) {
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() {
                "(vazio)".to_string()
            } else {
                t.to_string()
            }
        }
        Err(e) => format!("(indisponível: {})", e.lines().next().unwrap_or("").trim()),
    }
}

/// GPU/renderer — as linhas VGA/3D do `lspci`. Onde: seção HARDWARE.
/// Por que importa: a GUI é Slint (acelerada); "abre preto"/"não abre" costuma ser
/// driver/renderer, e sem isto o triador pergunta e espera o usuário responder.
/// **Saída:** uma linha por adaptador, ou o motivo de não ter dado.
pub(crate) fn gpu_info() -> String {
    if !has_bin("lspci") {
        return "(lspci ausente — instale pciutils p/ detalhar)".into();
    }
    let out = cmd_out_full("lspci", &[]);
    let linhas: Vec<&str> = out
        .lines()
        .filter(|l| {
            l.contains("VGA") || l.contains("3D controller") || l.contains("Display controller")
        })
        .collect();
    if linhas.is_empty() {
        "(nenhum adaptador VGA/3D listado)".into()
    } else {
        linhas.join(" | ")
    }
}
