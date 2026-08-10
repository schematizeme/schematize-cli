//! Instalação e versionamento das skills.
//! O quê: install/update/list/remove + estado das versões instaladas.
//! Onde: chamado por main a partir dos subcomandos. Usa curl/unzip (Linux).

use crate::registry::{self, Item};
use crate::util::{self, commands_dir, skills_dir, state_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub skills: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub version: String,
    pub installed_at: u64,
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

/// Resolve a última versão publicada seguindo o redirect do release "latest".
/// Fluxo: HEAD em .../latest/download/... → url efetiva contém /download/vX.Y.Z/.
pub fn resolve_latest(it: &Item) -> Result<String, String> {
    let url = registry::latest_zip_url(it);
    let eff = util::run("curl", &["-sIL", "-o", "/dev/null", "-w", "%{url_effective}", &url])?;
    parse_version(&eff).ok_or_else(|| format!("não consegui resolver a última versão de {}", it.slug))
}

/// Extrai "0.9.1" de uma url .../releases/download/v0.9.1/arquivo.zip.
fn parse_version(url: &str) -> Option<String> {
    let marker = "/download/v";
    let start = url.find(marker)? + marker.len();
    let rest = &url[start..];
    let end = rest.find('/')?;
    Some(rest[..end].to_string())
}

/// Instala (ou reinstala) uma skill a partir do release latest. Retorna a versão.
pub fn install(it: &Item) -> Result<String, String> {
    let url = registry::latest_zip_url(it);
    let tmp = util::run("mktemp", &["-d"])?.trim().to_string();
    let zip = format!("{tmp}/pkg.zip");
    let ex = format!("{tmp}/x");

    util::run("curl", &["-fSL", "-o", &zip, &url])?;
    util::run("unzip", &["-q", "-o", &zip, "-d", &ex])?;

    let extracted = Path::new(&ex).join(it.skill_dir);
    if !extracted.is_dir() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("zip de {} não contém {}/", it.slug, it.skill_dir));
    }
    let version = fs::read_to_string(extracted.join("VERSION"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "0.0.0".into());

    // Copia limpa: remove a versão anterior e recria (sem drift de arquivo).
    let dest = skills_dir().join(it.skill_dir);
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

    let mut st = load_state();
    st.skills.insert(it.slug.to_string(), Entry { version: version.clone(), installed_at: util::now_unix() });
    save_state(&st)?;
    Ok(version)
}

/// Remove uma skill instalada (pasta + registro). Comandos ficam (podem ser de outra origem).
pub fn remove(it: &Item) -> Result<(), String> {
    let dest = skills_dir().join(it.skill_dir);
    let _ = fs::remove_dir_all(&dest);
    let mut st = load_state();
    st.skills.remove(it.slug);
    save_state(&st)?;
    Ok(())
}

/// Linha de status por skill: instalada? versão? última disponível?
pub fn status_line(it: &Item, st: &State, check_remote: bool) -> String {
    let inst = st.skills.get(it.slug).map(|e| e.version.clone());
    let latest = if check_remote { resolve_latest(it).ok() } else { None };
    let inst_s = inst.clone().unwrap_or_else(|| "—".into());
    let up = match (&inst, &latest) {
        (Some(i), Some(l)) if i == l => "atual",
        (Some(_), Some(_)) => "ATUALIZAR",
        (None, Some(_)) => "não instalada",
        _ => "",
    };
    let latest_s = latest.unwrap_or_else(|| "?".into());
    format!("{:<12} instalada={:<8} latest={:<8} {}", it.slug, inst_s, latest_s, up)
}
