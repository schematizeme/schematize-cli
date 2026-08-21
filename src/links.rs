//! Links do ecossistema + comando `open`.
//! O quê: URLs canônicas (site, blog, github) e abertura no navegador.
//! Onde: usado por `schematize open|blog`, pela GUI e pelo status.

use crate::i18n::tf;
use crate::util::open_url;

pub const SITE: &str = "https://schematize.net";
pub const BLOG: &str = "https://blog.schematize.org";
/// Academy — cursos pagos e estruturados de como usar IA (videoaula).
pub const ACADEMY: &str = "https://academy.schematize.org";
/// Research — artigos densos, foco em disseminar conhecimento.
pub const RESEARCH: &str = "https://research.schematize.org";
pub const GITHUB: &str = "https://github.com/schematizeme";

/// Resolve um alvo textual para uma URL conhecida.
pub fn url_for(target: &str) -> Option<&'static str> {
    match target {
        "site" | "website" | "home" => Some(SITE),
        "blog" | "news" => Some(BLOG),
        "github" | "gh" | "repo" => Some(GITHUB),
        "academy" | "cursos" => Some(ACADEMY),
        "research" | "artigos" => Some(RESEARCH),
        _ => None,
    }
}

/// Abre um alvo (site/blog/github) no navegador. Erro se desconhecido.
pub fn open(target: &str) -> Result<(), String> {
    match url_for(target) {
        Some(u) => {
            println!("{}", tf("blog.opening", &[("url", u)]));
            open_url(u);
            Ok(())
        }
        None => Err(tf("open.unknown", &[("name", target)])),
    }
}
