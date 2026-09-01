//! Instalação e versionamento das skills.
//! O quê: install/update/list/remove + estado das versões instaladas.
//! Onde: chamado por main a partir dos subcomandos. Usa curl/unzip (Linux).

use crate::i18n;
use crate::registry::{self, Item};
use crate::util::{self, commands_dir, skills_dir, state_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

/// Serializa o read-modify-write de `state.json` — a GUI instala/remove em paralelo,
/// e sem isto dois `save_state` concorrentes perderiam a entrada um do outro.
static STATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub skills: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub version: String,
    pub installed_at: u64,
    /// A skill virou um FORK editável? Skills OFICIAIS (do catálogo) não são editadas direto:
    /// na 1ª edição vira fork (cópia ativa editável + base oficial guardada no stash pra comparar).
    /// `#[serde(default)]` = estado antigo (sem o campo) ainda parseia como `false`.
    #[serde(default)]
    pub forked: bool,
    /// Versão oficial em que o fork foi baseado (o que estava instalado no momento do fork).
    /// Serve de âncora pro `compare_update` mostrar "base vNN → nova oficial vNN".
    #[serde(default)]
    pub fork_base_version: Option<String>,
}

/// Lê o estado (versões instaladas); vazio se não existir.
pub fn load_state() -> State {
    match fs::read_to_string(state_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => State::default(),
    }
}

/// Grava o estado de forma atômica-ish (cria o diretório se preciso).
pub fn save_state(st: &State) -> Result<(), String> {
    let p = state_path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(st).map_err(|e| e.to_string())?;
    fs::write(&p, body).map_err(|e| e.to_string())
}

/// Versão instalada lida do disco (`~/.claude/skills/<dir>/VERSION`) — fonte de verdade,
/// independente de como a skill foi instalada (CLI, install.sh, unzip). None se ausente.
pub fn installed_version(it: &Item) -> Option<String> {
    fs::read_to_string(skills_dir().join(&it.skill_dir).join("VERSION"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve a última versão de uma skill/CLI a partir do FONTE (raw main) — NÃO da API
/// do GitHub (que é limitada a 60/h e por isso quebrava o versionamento). Ver `latest_version_raw`.
pub fn resolve_latest(it: &Item) -> Result<String, String> {
    latest_version_raw(&it.repo)
        .ok_or_else(|| format!("não consegui resolver a versão de {}", it.slug))
}

/// Última versão lida de `raw.githubusercontent.com/<org>/<repo>/main/...` — SEM a API
/// (evita o teto de 60/h que zerava a detecção). Skills têm `VERSION` no root; a CLI tem a
/// versão no `Cargo.toml`. raw tem cache curto e limite muito maior; casa com o source-first.
pub fn latest_version_raw(repo: &str) -> Option<String> {
    let is_cli = repo == "schematize-cli";
    let file = if is_cli { "Cargo.toml" } else { "VERSION" };
    let url = format!("https://raw.githubusercontent.com/{}/{}/main/{}", registry::ORG, repo, file);
    let body =
        util::run("curl", &["-sfL", "-m", "8", "-H", "User-Agent: schematize-cli", &url]).ok()?;
    if is_cli {
        // primeira linha `version = "x.y.z"` do [package]
        body.lines()
            .map(|l| l.trim())
            .find(|l| l.starts_with("version") && l.contains('='))
            .and_then(|l| l.split('"').nth(1))
            .map(|s| s.to_string())
    } else {
        let v = body.trim().to_string();
        (!v.is_empty() && v.chars().next().is_some_and(|c| c.is_ascii_digit())).then_some(v)
    }
}

/// TODAS as versões numa ÚNICA chamada, pelo AGREGADOR da API (`GET /versions`).
///
/// O quê: pergunta `{api}/versions?skills=a,b,c` e devolve `slug -> versão`. Existe pra matar
/// o N+1 do sininho (era 1 ida ao GitHub POR skill) e, mais importante, pra app e SITE lerem a
/// MESMA fonte: o espelho da API, que o `sync-skills` mantém a partir dos releases do GitHub.
/// Onde: `notifications::collect` (checagem de desatualizadas).
/// Postura: best-effort — API fora do ar/resposta inválida → mapa VAZIO, e o chamador cai no
/// caminho antigo (raw por skill). Nunca panica, nunca bloqueia mais que 8s.
pub fn latest_versions_bulk(slugs: &[String]) -> BTreeMap<String, String> {
    if slugs.is_empty() {
        return BTreeMap::new();
    }
    let url = format!("{}/versions?skills={}", crate::account::api_base(), slugs.join(","));
    let Ok(body) =
        util::run("curl", &["-sfL", "-m", "8", "-H", "User-Agent: schematize-cli", &url])
    else {
        return BTreeMap::new();
    };
    parse_versions(&body)
}

/// Parse PURO da resposta do agregador (`{"count":N,"data":{"go":"1.9.2"}}`).
///
/// O quê: extrai o mapa `slug -> versão`, ignorando entradas não-string. JSON inválido ou sem
/// `data` → mapa vazio (o chamador trata como "API não respondeu").
/// Onde: `latest_versions_bulk`; testado sem rede.
fn parse_versions(json: &str) -> BTreeMap<String, String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return BTreeMap::new();
    };
    let Some(map) = v.get("data").and_then(|d| d.as_object()) else {
        return BTreeMap::new();
    };
    map.iter()
        .filter_map(|(k, val)| {
            let s = val.as_str()?.trim();
            (!s.is_empty()).then(|| (k.clone(), s.to_string()))
        })
        .collect()
}

/// (Fallback/diagnóstico) última versão via API do GitHub (`releases/latest` → `tag_name`),
/// sem "v". Limitada a 60/h/IP — por isso NÃO é mais o caminho primário de detecção.
pub fn latest_release_tag(repo: &str) -> Option<String> {
    let url = format!("https://api.github.com/repos/{}/{}/releases/latest", registry::ORG, repo);
    let body = util::run(
        "curl",
        &[
            "-sfL",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: schematize-cli",
            &url,
        ],
    )
    .ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let tag = v.get("tag_name")?.as_str()?;
    Some(tag.trim_start_matches('v').to_string())
}

/// Instala (ou reinstala) uma skill a partir do release latest. Retorna a versão.
pub fn install(it: &Item) -> Result<String, String> {
    let url = registry::latest_zip_url(it);
    let tmp = util::run("mktemp", &["-d"])?.trim().to_string();
    let zip = format!("{tmp}/pkg.zip");
    let ex = format!("{tmp}/x");

    util::run("curl", &["-fSL", "-o", &zip, &url])?;
    util::run("unzip", &["-q", "-o", &zip, "-d", &ex])?;

    let extracted = Path::new(&ex).join(&it.skill_dir);
    if !extracted.is_dir() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("zip de {} não contém {}/", it.slug, it.skill_dir));
    }
    let version = fs::read_to_string(extracted.join("VERSION"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "0.0.0".into());

    // Copia limpa: remove a versão anterior e recria (sem drift de arquivo).
    let dest = skills_dir().join(&it.skill_dir);
    fs::create_dir_all(skills_dir()).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&dest);
    util::run("cp", &["-r", extracted.to_str().unwrap(), dest.to_str().unwrap()])?;

    // Comandos achatados em ~/.claude/commands (nomes já são <slug>-*, únicos).
    let cmd_src = dest.join("assets").join("commands");
    if cmd_src.is_dir() {
        fs::create_dir_all(commands_dir()).map_err(|e| e.to_string())?;
        for e in fs::read_dir(&cmd_src).map_err(|e| e.to_string())? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.extension().and_then(|s| s.to_str()) == Some("md") {
                let name = p.file_name().unwrap();
                fs::copy(&p, commands_dir().join(name)).map_err(|e| e.to_string())?;
            }
        }
    }

    let _ = fs::remove_dir_all(&tmp);

    {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = load_state();
        // Reinstalar do oficial ZERA o estado de fork da entrada (a pasta ativa volta a ser oficial).
        st.skills.insert(
            it.slug.to_string(),
            Entry {
                version: version.clone(),
                installed_at: util::now_unix(),
                forked: false,
                fork_base_version: None,
            },
        );
        save_state(&st)?;
    }
    Ok(version)
}

/// Remove uma skill instalada: apaga os comandos achatados dela (nomes `<slug>-*` únicos),
/// a pasta da skill e o registro no estado. Uninstall completo.
pub fn remove(it: &Item) -> Result<(), String> {
    let dest = skills_dir().join(&it.skill_dir);
    // Apaga os comandos desta skill de ~/.claude/commands ANTES de remover a pasta
    // (a lista de nomes vem de assets/commands da própria skill; nomes são globalmente únicos).
    let cmd_src = dest.join("assets").join("commands");
    if let Ok(rd) = fs::read_dir(&cmd_src) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Some(name) = p.file_name() {
                    let _ = fs::remove_file(commands_dir().join(name));
                }
            }
        }
    }
    let _ = fs::remove_dir_all(&dest);
    {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = load_state();
        st.skills.remove(&it.slug);
        save_state(&st)?;
    }
    Ok(())
}

/// Linha de status por skill: instalada? versão? última disponível? é um fork?
pub fn status_line(it: &Item, st: &State, check_remote: bool) -> String {
    let inst = installed_version(it);
    let latest = if check_remote { resolve_latest(it).ok() } else { None };
    let inst_s = inst.clone().unwrap_or_else(|| "—".into());
    let forked = st.skills.get(&it.slug).map(|e| e.forked).unwrap_or(false);
    let up = match (&inst, &latest) {
        // Skill forkada não "atualiza" no sentido normal — sinaliza pra comparar/mesclar.
        _ if forked => "[fork]".to_string(),
        (Some(i), Some(l)) if i == l => i18n::t("common.current"),
        (Some(_), Some(_)) => i18n::t("common.update"),
        (None, Some(_)) => i18n::t("common.not_installed"),
        _ => String::new(),
    };
    let latest_s = latest.unwrap_or_else(|| "?".into());
    format!("{:<12} {:<8} latest={:<8} {}", it.slug, inst_s, latest_s, up)
}

// ------------------------------------------------------------------------------------------------
// Modelo de FORK — skills oficiais viram cópia editável + base oficial guardada pra comparar.
// ------------------------------------------------------------------------------------------------

/// Diretório do STASH das bases oficiais: `~/.schematize/skill-base/`. Guarda a versão oficial
/// em que cada fork foi baseado (só pra `compare_update` — nunca é a pasta ativa do Claude).
pub fn base_stash_dir() -> std::path::PathBuf {
    util::home_app_dir().join("skill-base")
}

/// Um slug é OFICIAL se está no catálogo/registry (skill da casa/parceiro vetado). Skills
/// criadas pelo usuário (fora do catálogo) NÃO são oficiais e editam livremente (sem fork).
pub fn is_official(slug: &str) -> bool {
    registry::find(&registry::catalog(), slug).is_some()
}

/// Lê a versão de uma pasta de skill (`<dir>/VERSION`), se presente e não-vazia.
fn version_at(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join("VERSION"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Cópia recursiva de uma pasta (destino limpo antes) via `cp -r` — mesmo mecanismo do `install`.
fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_dir_all(dst);
    util::run(
        "cp",
        &["-r", src.to_str().ok_or("path inválido")?, dst.to_str().ok_or("path inválido")?],
    )?;
    Ok(())
}

/// Núcleo TESTÁVEL do fork: recebe as raízes (skills e stash) explicitamente, o estado e se o
/// slug é oficial. Idempotente: se já forkada (ou não-oficial) não faz nada. Quando forka, copia
/// `schematize-<slug>/` da raiz de skills pro stash da base e marca `forked`+`fork_base_version`
/// na entrada de estado. Retorna `true` se ESTA chamada criou o fork. NÃO grava o estado (o chamador grava).
fn ensure_fork(
    skills_root: &Path,
    base_root: &Path,
    st: &mut State,
    slug: &str,
    official: bool,
) -> Result<bool, String> {
    // Não-oficial ou já forkada → nada a fazer (a pasta ativa já é editável).
    if !official {
        return Ok(false);
    }
    if st.skills.get(slug).map(|e| e.forked).unwrap_or(false) {
        return Ok(false);
    }
    let src = skills_root.join(format!("schematize-{slug}"));
    if !src.is_dir() {
        return Err(format!("skill não instalada: {}", src.display()));
    }
    // Guarda a base oficial no stash (pra comparar depois). A pasta ativa continua sendo o fork.
    copy_tree(&src, &base_root.join(slug))?;
    let inst = version_at(&src);
    let entry = st.skills.entry(slug.to_string()).or_insert_with(|| Entry {
        version: inst.clone().unwrap_or_default(),
        installed_at: util::now_unix(),
        forked: false,
        fork_base_version: None,
    });
    entry.forked = true;
    entry.fork_base_version = inst;
    Ok(true)
}

/// Transforma uma skill oficial em FORK editável (idempotente): copia a pasta ativa pro stash da
/// base oficial (`~/.schematize/skill-base/<slug>/`), marca `forked=true` e `fork_base_version`.
/// Se a skill não é oficial ou já está forkada, é um no-op. A pasta ativa NÃO é renomeada/desativada
/// no Claude — ela É o fork; o stash serve só pra `compare_update`.
pub fn fork(slug: &str) -> Result<(), String> {
    let official = is_official(slug);
    let _guard = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut st = load_state();
    if ensure_fork(&skills_dir(), &base_stash_dir(), &mut st, slug, official)? {
        save_state(&st)?;
    }
    Ok(())
}

/// Gancho do editor: chamado ANTES de gravar um arquivo de uma skill. Se o slug é oficial e ainda
/// não virou fork, forka (garante o stash da base) — assim nenhum dado oficial se perde na 1ª edição.
/// Skills não-oficiais passam direto. Idempotente (a partir da 2ª edição não faz nada).
pub fn fork_on_edit(slug: &str) -> Result<(), String> {
    if is_official(slug) {
        fork(slug)?;
    }
    Ok(())
}

/// Status de UM arquivo na comparação fork-ativo vs nova oficial.
pub struct FileDiff {
    /// Caminho relativo à raiz da skill (com `/`).
    pub path: String,
    /// "igual" | "mudou" | "novo" (só no fork) | "removido" (só na nova oficial).
    pub status: String,
}

/// Resultado da comparação entre o FORK ativo e a nova versão OFICIAL baixada.
pub struct Compare {
    /// Versão oficial em que o fork foi baseado (do estado).
    pub base_version: String,
    /// Versão da nova oficial baixada pra comparar.
    pub new_version: String,
    /// Arquivo-a-arquivo (ordenado por caminho).
    pub files: Vec<FileDiff>,
    /// Diff unificado legível (fork ativo → nova oficial). Pode ser vazio se idênticos.
    pub diff_text: String,
}

/// Lista recursivamente os arquivos de uma pasta como caminhos relativos (`/`-separados), ordenados.
fn list_tree(root: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(base, &p, out);
                } else if p.is_file() {
                    if let Ok(rel) = p.strip_prefix(base) {
                        out.push(rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Roda `diff -ruN a b` capturando o stdout MESMO quando há diferenças (exit 1). Só exit ≥2
/// (erro real do diff) vira `Err`. Se o `diff` do sistema faltar, retorna aviso (não quebra o Compare).
fn unified_diff(a: &Path, b: &Path) -> String {
    use std::process::Command;
    match Command::new("diff").args(["-ruN", &a.to_string_lossy(), &b.to_string_lossy()]).output() {
        Ok(out) => {
            let code = out.status.code().unwrap_or(0);
            if code >= 2 {
                format!("(diff indisponível: {})", String::from_utf8_lossy(&out.stderr).trim())
            } else {
                String::from_utf8_lossy(&out.stdout).into_owned()
            }
        }
        Err(_) => "(comando `diff` do sistema indisponível)".to_string(),
    }
}

/// Compara o FORK ativo de uma skill com a NOVA versão oficial (latest). Baixa a oficial num
/// tempdir (mesmo mecanismo de download/zip do `install`, SEM instalar por cima) e monta o
/// relatório: `files` (igual/mudou/novo/removido) + `diff_text` unificado. Não sobrescreve nada.
/// Erro se a skill não é um fork (nada a comparar) ou se não está no catálogo.
pub fn compare_update(slug: &str) -> Result<Compare, String> {
    // 1) A skill precisa ser um fork registrado no estado.
    let st = load_state();
    let entry = st.skills.get(slug).ok_or_else(|| format!("skill {slug} não está instalada"))?;
    if !entry.forked {
        return Err(format!(
            "skill {slug} não é um fork — nada a comparar (edite-a pra criar o fork)"
        ));
    }
    let base_version = entry.fork_base_version.clone().unwrap_or_else(|| "?".into());

    // 2) Resolve o Item no catálogo pra saber onde baixar.
    let it = registry::find(&registry::catalog(), slug)
        .ok_or_else(|| format!("skill {slug} não está no catálogo oficial"))?;

    // 3) Baixa a nova oficial num tempdir (curl + unzip), SEM instalar por cima.
    let url = registry::latest_zip_url(&it);
    let tmp = util::run("mktemp", &["-d"])?.trim().to_string();
    let zip = format!("{tmp}/pkg.zip");
    let ex = format!("{tmp}/x");
    let dl = (|| -> Result<std::path::PathBuf, String> {
        util::run("curl", &["-fSL", "-o", &zip, &url])?;
        util::run("unzip", &["-q", "-o", &zip, "-d", &ex])?;
        let extracted = Path::new(&ex).join(&it.skill_dir);
        if !extracted.is_dir() {
            return Err(format!("zip de {slug} não contém {}/", it.skill_dir));
        }
        Ok(extracted)
    })();
    let official = match dl {
        Ok(p) => p,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    let new_version = version_at(&official).unwrap_or_else(|| "?".into());

    // 4) Compara arquivo-a-arquivo: fork ativo vs nova oficial.
    let fork_dir = skills_dir().join(&it.skill_dir);
    let fork_files = list_tree(&fork_dir);
    let new_files = list_tree(&official);
    let mut files: Vec<FileDiff> = Vec::new();
    let mut all: Vec<String> = fork_files.iter().chain(new_files.iter()).cloned().collect();
    all.sort();
    all.dedup();
    for rel in all {
        let in_fork = fork_files.contains(&rel);
        let in_new = new_files.contains(&rel);
        let status = match (in_fork, in_new) {
            (true, true) => {
                let a = fs::read(fork_dir.join(&rel)).unwrap_or_default();
                let b = fs::read(official.join(&rel)).unwrap_or_default();
                if a == b {
                    "igual"
                } else {
                    "mudou"
                }
            }
            (true, false) => "novo", // existe só no fork (o usuário adicionou)
            (false, true) => "removido", // existe só na nova oficial (fork removeu / upstream adicionou)
            (false, false) => continue,
        };
        files.push(FileDiff { path: rel, status: status.to_string() });
    }

    let diff_text = unified_diff(&fork_dir, &official);
    let _ = fs::remove_dir_all(&tmp);

    Ok(Compare { base_version, new_version, files, diff_text })
}

/// Atualiza uma skill instalada pro latest oficial — MAS recusa se ela é um fork (não sobrescreve
/// o trabalho do usuário). Pra fork, a via é `compare_update` + mesclar à mão (a GUI oferece o diff).
/// Pra skill não-forkada, delega ao `install` (cópia limpa do release). Retorna a versão instalada.
pub fn update(it: &Item) -> Result<String, String> {
    let st = load_state();
    if st.skills.get(&it.slug).map(|e| e.forked).unwrap_or(false) {
        return Err(format!(
            "skill {} é um fork — não sobrescrevo. Use `schematize skills compare {}` pra ver o diff e mesclar.",
            it.slug, it.slug
        ));
    }
    install(it)
}

// ------------------------------------------------------------------------------------------------
// Testes: núcleos injetáveis (tempdir) — não tocam em $HOME nem na rede.
// ------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::parse_versions;

    /// O agregador devolve `{count, data:{slug: versão}}` — o parse extrai só o mapa.
    #[test]
    fn parse_versions_extrai_o_mapa() {
        let m = parse_versions(r#"{"count":2,"data":{"go":"1.9.2","rust":"1.9.2"}}"#);
        assert_eq!(m.get("go").map(String::as_str), Some("1.9.2"));
        assert_eq!(m.get("rust").map(String::as_str), Some("1.9.2"));
        assert_eq!(m.len(), 2);
    }

    /// API fora do ar/resposta lixo => mapa VAZIO (o chamador cai no fallback por skill),
    /// nunca panica e nunca inventa versão.
    #[test]
    fn parse_versions_resiliente_a_lixo() {
        for lixo in ["", "não é json", "{}", r#"{"data":null}"#, r#"{"data":{"go":123}}"#] {
            assert!(parse_versions(lixo).is_empty(), "deveria ser vazio: {lixo:?}");
        }
    }

    use super::*;

    /// Tempdir único por pid + contador (paralelo sem colisão).
    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let id = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("schematize_skills_{}_{id}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// Cria uma skill sintética `schematize-<slug>/` (com VERSION + 1 arquivo) sob `skills_root`.
    fn mk_skill(skills_root: &Path, slug: &str, version: &str) -> std::path::PathBuf {
        let dir = skills_root.join(format!("schematize-{slug}"));
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(dir.join("VERSION"), format!("{version}\n")).unwrap();
        fs::write(dir.join("SKILL.md"), "corpo original").unwrap();
        fs::write(dir.join("references/a.md"), "ref a").unwrap();
        dir
    }

    #[test]
    fn ensure_fork_idempotente_e_cria_stash() {
        let root = tmp();
        let skills_root = root.join("skills");
        let base_root = root.join("base");
        mk_skill(&skills_root, "demo", "1.2.3");
        let mut st = State::default();

        // 1ª chamada (oficial): forka de fato, copia a base pro stash e marca o estado.
        let did = ensure_fork(&skills_root, &base_root, &mut st, "demo", true).unwrap();
        assert!(did, "a primeira chamada deve forkar");
        assert!(base_root.join("demo").join("VERSION").is_file(), "stash da base foi criado");
        assert_eq!(fs::read_to_string(base_root.join("demo/SKILL.md")).unwrap(), "corpo original");
        let e = st.skills.get("demo").unwrap();
        assert!(e.forked);
        assert_eq!(e.fork_base_version.as_deref(), Some("1.2.3"));

        // 2ª chamada: idempotente (não forka de novo).
        let did2 = ensure_fork(&skills_root, &base_root, &mut st, "demo", true).unwrap();
        assert!(!did2, "segunda chamada é no-op");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ensure_fork_ignora_nao_oficial() {
        let root = tmp();
        let skills_root = root.join("skills");
        let base_root = root.join("base");
        mk_skill(&skills_root, "minha", "0.1.0");
        let mut st = State::default();

        // Skill não-oficial NÃO forka (edita livremente) e não cria stash.
        let did = ensure_fork(&skills_root, &base_root, &mut st, "minha", false).unwrap();
        assert!(!did);
        assert!(!base_root.join("minha").exists());
        assert!(!st.skills.get("minha").map(|e| e.forked).unwrap_or(false));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ensure_fork_erra_se_skill_ausente() {
        let root = tmp();
        let mut st = State::default();
        // Oficial mas não instalada em disco → erro claro (nada a copiar).
        assert!(
            ensure_fork(&root.join("skills"), &root.join("base"), &mut st, "sumida", true).is_err()
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn is_official_usa_o_catalogo() {
        // "engineering" existe no catálogo (remoto OU no fallback embutido).
        assert!(is_official("engineering"));
        // Um slug improvável não está no catálogo.
        assert!(!is_official("slug-que-nao-existe-xyz-123"));
    }

    #[test]
    fn list_tree_relativo_e_ordenado() {
        let root = tmp();
        let dir = mk_skill(&root, "lt", "0.1.0");
        let files = list_tree(&dir);
        assert!(files.contains(&"VERSION".to_string()));
        assert!(files.contains(&"SKILL.md".to_string()));
        assert!(files.contains(&"references/a.md".to_string()));
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted, "determinístico (ordenado)");
        fs::remove_dir_all(&root).ok();
    }
}
