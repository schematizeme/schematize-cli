//! i18n — catálogo de strings multi-idioma da UI (CLI + GUI).
//! O quê: resolve o idioma ativo (config → env → fallback en) e traduz chaves.
//! Onde: `t()`/`tf()` usados por main/gui/agent/doctor/status/news/upgrade.
//! Os JSONs por idioma vivem em `src/i18n/<code>.json` e são embutidos no binário.

use crate::config;
use std::collections::HashMap;
use std::sync::RwLock;

/// Idiomas suportados: (código, nome nativo, JSON embutido).
/// Adicionar idioma = um `.json` + uma linha aqui.
pub const LANGS: &[(&str, &str, &str)] = &[
    ("en", "English",       include_str!("i18n/en.json")),
    ("es", "Español",       include_str!("i18n/es.json")),
    ("it", "Italiano",      include_str!("i18n/it.json")),
    ("fr", "Français",      include_str!("i18n/fr.json")),
    ("de", "Deutsch",       include_str!("i18n/de.json")),
    ("pt", "Português",     include_str!("i18n/pt.json")),
    ("ja", "日本語",         include_str!("i18n/ja.json")),
    ("zh", "中文",           include_str!("i18n/zh.json")),
    ("ru", "Русский",       include_str!("i18n/ru.json")),
    ("ar", "العربية",       include_str!("i18n/ar.json")),
    ("hi", "हिन्दी",          include_str!("i18n/hi.json")),
];

/// Idioma ativo já parseado (mapa do idioma + mapa en para fallback).
struct Active {
    code: String,
    map: HashMap<String, String>,
    en: HashMap<String, String>,
}

static ACTIVE: RwLock<Option<Active>> = RwLock::new(None);

/// True se o código é um idioma que suportamos.
pub fn is_supported(code: &str) -> bool {
    LANGS.iter().any(|(c, _, _)| *c == code)
}

/// Nome nativo do idioma, se suportado.
pub fn name_of(code: &str) -> Option<&'static str> {
    LANGS.iter().find(|(c, _, _)| *c == code).map(|(_, n, _)| *n)
}

/// JSON embutido de um código (ou en como fallback).
fn json_of(code: &str) -> &'static str {
    LANGS.iter().find(|(c, _, _)| *c == code).map(|(_, _, j)| *j)
        .unwrap_or(LANGS[0].2)
}

fn parse(code: &str) -> HashMap<String, String> {
    serde_json::from_str(json_of(code)).unwrap_or_default()
}

/// Normaliza um valor de env tipo "pt_BR.UTF-8" → "pt" (se suportado).
fn from_env_value(v: &str) -> Option<String> {
    let two: String = v.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let two = two.to_lowercase();
    if two.len() >= 2 {
        let code = &two[..2];
        if is_supported(code) {
            return Some(code.to_string());
        }
    }
    None
}

/// Resolve o idioma ativo: config.json → $SCHEMATIZE_LANG → $LANG/$LC_* → en.
pub fn resolve_code() -> String {
    if let Some(c) = config::load().lang {
        if is_supported(&c) {
            return c;
        }
    }
    for var in ["SCHEMATIZE_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if let Some(c) = from_env_value(&v) {
                return c;
            }
        }
    }
    "en".to_string()
}

/// Garante o idioma ativo carregado (idempotente).
fn ensure() {
    {
        if ACTIVE.read().unwrap().is_some() {
            return;
        }
    }
    let code = resolve_code();
    let a = Active { map: parse(&code), en: parse("en"), code };
    *ACTIVE.write().unwrap() = Some(a);
}

/// Troca o idioma ativo em runtime (GUI) e persiste na config.
pub fn set_lang(code: &str) -> Result<(), String> {
    if !is_supported(code) {
        return Err(format!("idioma não suportado: {code}"));
    }
    config::set_lang(code)?;
    let a = Active { map: parse(code), en: parse("en"), code: code.to_string() };
    *ACTIVE.write().unwrap() = Some(a);
    Ok(())
}

/// Código do idioma ativo.
pub fn current_code() -> String {
    ensure();
    ACTIVE.read().unwrap().as_ref().map(|a| a.code.clone()).unwrap_or_else(|| "en".into())
}

/// Traduz uma chave. Fallback: idioma ativo → en → a própria chave.
pub fn t(key: &str) -> String {
    ensure();
    let g = ACTIVE.read().unwrap();
    let a = g.as_ref().unwrap();
    a.map.get(key).or_else(|| a.en.get(key)).cloned().unwrap_or_else(|| key.to_string())
}

/// Traduz e substitui placeholders `{nome}` pelos valores dados.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    let mut s = t(key);
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}
