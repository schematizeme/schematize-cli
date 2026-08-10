//! `schematize news` — últimos posts do blog (blog.schematize.net).
//! O quê: busca o feed RSS/Atom (best-effort), lista títulos+links e detecta
//! posts novos pro agente notificar. Onde: `schematize news` e agent::run_*.
//! Sem crate de XML: parser tolerante o suficiente pra RSS e Atom comuns.

use crate::i18n::{t, tf};
use crate::links::BLOG;
use crate::util;
use std::fs;

/// URLs de feed tentadas em ordem (cobre geradores comuns: Ghost, Hugo, WP…).
const FEEDS: &[&str] = &[
    "https://blog.schematize.net/rss/",
    "https://blog.schematize.net/rss.xml",
    "https://blog.schematize.net/feed/",
    "https://blog.schematize.net/feed.xml",
    "https://blog.schematize.net/atom.xml",
    "https://blog.schematize.net/index.xml",
];

/// Um post do feed.
pub struct Post {
    pub title: String,
    pub link: String,
}

/// Marcador do último post visto (pro agente não repetir notificação).
fn marker_path() -> std::path::PathBuf {
    util::config_path().with_file_name("last_post.txt")
}

/// Baixa o primeiro feed que responder com corpo não-vazio.
fn fetch_feed() -> Option<String> {
    for u in FEEDS {
        if let Ok(body) = util::run("curl", &["-sfL", "-m", "8", "-H", "User-Agent: schematize-cli", u]) {
            if body.contains("<item") || body.contains("<entry") {
                return Some(body);
            }
        }
    }
    None
}

/// Extrai o conteúdo da primeira tag `<tag ...>...</tag>` de um trecho.
fn between(chunk: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let s = chunk.find(&open)?;
    let gt = chunk[s..].find('>')? + s + 1;
    let e = chunk[gt..].find(&close)? + gt;
    Some(clean(&chunk[gt..e]))
}

/// Limpa CDATA e espaços de um valor textual.
fn clean(s: &str) -> String {
    s.replace("<![CDATA[", "").replace("]]>", "").trim().to_string()
}

/// Link de um item: `<link>URL</link>` (RSS) ou `<link href="URL"` (Atom).
fn link_of(chunk: &str) -> Option<String> {
    if let Some(l) = between(chunk, "link") {
        if l.starts_with("http") {
            return Some(l);
        }
    }
    let s = chunk.find("<link")?;
    let h = chunk[s..].find("href=\"")? + s + 6;
    let e = chunk[h..].find('"')? + h;
    Some(chunk[h..e].to_string())
}

/// Faz o parse do feed em posts (RSS `<item>` ou Atom `<entry>`), até `n`.
fn parse(body: &str, n: usize) -> Vec<Post> {
    let sep = if body.contains("<item") { "<item" } else { "<entry" };
    body.split(sep)
        .skip(1)
        .filter_map(|chunk| {
            let title = between(chunk, "title")?;
            let link = link_of(chunk).unwrap_or_else(|| BLOG.to_string());
            if title.is_empty() {
                None
            } else {
                Some(Post { title, link })
            }
        })
        .take(n)
        .collect()
}

/// Retorna até `n` posts recentes (vazio se sem feed).
pub fn latest(n: usize) -> Vec<Post> {
    fetch_feed().map(|b| parse(&b, n)).unwrap_or_default()
}

/// `schematize news`: lista os últimos posts no terminal.
pub fn show() {
    println!("{}", t("news.title"));
    println!("{}", t("news.fetching"));
    let posts = latest(5);
    if posts.is_empty() {
        println!("{}", t("news.none"));
        println!("{}", tf("news.more", &[("url", BLOG)]));
        return;
    }
    for p in &posts {
        println!("  • {}\n    {}", p.title, p.link);
    }
    println!("\n{}", tf("news.more", &[("url", BLOG)]));
}

/// Pro agente: há post novo desde a última checagem? Retorna o link do mais recente.
/// Atualiza o marcador quando encontra algo novo.
pub fn check_new() -> Option<String> {
    let posts = latest(1);
    let newest = posts.first()?;
    let seen = fs::read_to_string(marker_path()).unwrap_or_default();
    if seen.trim() == newest.link {
        return None;
    }
    let _ = fs::write(marker_path(), &newest.link);
    Some(newest.link.clone())
}
