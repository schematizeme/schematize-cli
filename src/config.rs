//! Config do usuário (idioma) em `~/.claude/schematize/config.json`.
//! O quê: lê/grava a preferência de idioma. Onde: usado pelo i18n e por `lang`.

use crate::util::config_path;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    /// Código do idioma escolhido (ex.: "pt", "en"). None = auto (env/fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Projetos abertos recentemente na GUI (caminhos absolutos, mais recente primeiro).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_projects: Vec<String>,
    /// Diretórios de desenvolvimento cadastrados: cada um pode ser a pasta de UM
    /// projeto ou uma pasta guarda-chuva (ex.: `PROJETOS`) onde o schematize
    /// descobre os projetos pelos marcadores. Caminhos absolutos.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dev_dirs: Vec<String>,
    /// Projetos FIXADOS manualmente (pin): sempre listados na varredura, mesmo sem
    /// marcador git, e param a descida (um guarda-chuva pinado vira UM projeto).
    /// Caminhos absolutos (canônicos quando o diretório existe no momento do pin).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}

/// Lê a config (vazia se não existir/ inválida).
pub fn load() -> Config {
    match fs::read_to_string(config_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Grava a config (cria o diretório se preciso).
pub fn save(c: &Config) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(c).map_err(|e| e.to_string())?;
    fs::write(&p, body).map_err(|e| e.to_string())
}

/// Persiste só o idioma, preservando o resto.
pub fn set_lang(code: &str) -> Result<(), String> {
    let mut c = load();
    c.lang = Some(code.to_string());
    save(&c)
}

/// Lista de projetos recentes (mais recente primeiro).
pub fn recent_projects() -> Vec<String> {
    load().recent_projects
}

/// Registra um projeto como o mais recente (dedup, teto de 12). Persiste.
pub fn add_recent_project(path: &str) {
    let mut c = load();
    c.recent_projects.retain(|p| p != path);
    c.recent_projects.insert(0, path.to_string());
    c.recent_projects.truncate(12);
    let _ = save(&c);
}

/// Diretórios de desenvolvimento cadastrados (na ordem de cadastro).
pub fn dev_dirs() -> Vec<String> {
    load().dev_dirs
}

/// Cadastra um diretório de desenvolvimento (dedup exato, mantém ordem). Persiste.
pub fn add_dev_dir(path: &str) {
    let mut c = load();
    if !c.dev_dirs.iter().any(|p| p == path) {
        c.dev_dirs.push(path.to_string());
        let _ = save(&c);
    }
}

/// Remove um diretório de desenvolvimento cadastrado. Persiste.
pub fn remove_dev_dir(path: &str) {
    let mut c = load();
    let before = c.dev_dirs.len();
    c.dev_dirs.retain(|p| p != path);
    if c.dev_dirs.len() != before {
        let _ = save(&c);
    }
}

/// Projetos fixados (pin), na ordem de cadastro.
pub fn projects() -> Vec<String> {
    load().projects
}

/// Fixa um projeto (pin). Canonicaliza se o diretório existir; dedup exato. Persiste.
/// Torna a pasta um projeto de primeira classe na varredura, sem depender de
/// heurística de `.git`/marcador (ideal pra listar um guarda-chuva como UM projeto).
pub fn pin_project(path: &str) {
    let canon = std::fs::canonicalize(path)
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| path.to_string());
    let mut c = load();
    if !c.projects.iter().any(|p| p == &canon) {
        c.projects.push(canon);
        let _ = save(&c);
    }
}

/// Desafixa um projeto (unpin). Remove por match do caminho canônico OU literal
/// (assim funciona mesmo se o pin foi salvo canônico e o usuário passa o relativo,
/// ou vice-versa). Persiste se removeu algo.
pub fn unpin_project(path: &str) {
    let canon = std::fs::canonicalize(path).ok().and_then(|p| p.to_str().map(String::from));
    let mut c = load();
    let before = c.projects.len();
    c.projects.retain(|p| p != path && canon.as_deref() != Some(p.as_str()));
    if c.projects.len() != before {
        let _ = save(&c);
    }
}
