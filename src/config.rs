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
