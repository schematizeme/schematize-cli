//! Catálogo de skills — ÍNDICE REMOTO com fallback embutido.
//! O quê: a lista de skills instaláveis vem de um `catalog.json` remoto (fonte de
//! verdade), pra skills novas aparecerem SEM recompilar/relançar a CLI. Se a rede
//! falhar, cai no catálogo embutido. Onde: consultado por install/update/list/agent/gui.

use crate::util;
use serde::Deserialize;

/// Org GitHub que hospeda o catálogo e as skills.
pub const ORG: &str = "schematizeme";

/// Índice remoto (raw do repo da CLI, branch main) — editar/commitar = catálogo novo.
const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/schematizeme/schematize-cli/main/catalog.json";

/// Uma skill/tool instalável do ecossistema.
#[derive(Clone, Deserialize)]
pub struct Item {
    /// slug curto (o que o usuário digita): "engineering", "seo", ...
    pub slug: String,
    /// nome da pasta instalada em ~/.claude/skills: "schematize-<slug>".
    pub skill_dir: String,
    /// repositório GitHub: "skill-<slug>".
    pub repo: String,
    /// nome do asset .zip do release.
    pub zip: String,
}

#[derive(Deserialize)]
struct Catalog {
    skills: Vec<Item>,
}

/// Catálogo embutido — fallback quando o índice remoto está indisponível (offline).
/// Mantido em dia como rede de segurança; a fonte de verdade é o `catalog.json`.
fn builtin() -> Vec<Item> {
    const B: &[(&str, &str, &str, &str)] = &[
        ("engineering", "schematize-engineering", "skill-engineering", "skill-engineering.zip"),
        ("go", "schematize-go", "skill-go", "skill-go.zip"),
        ("rust", "schematize-rust", "skill-rust", "skill-rust.zip"),
        ("web", "schematize-web", "skill-web", "skill-web.zip"),
        ("seo", "schematize-seo", "skill-seo", "skill-seo.zip"),
        ("node", "schematize-node", "skill-node", "skill-node.zip"),
        ("pentest", "schematize-pentest", "skill-pentest", "skill-pentest.zip"),
    ];
    B.iter()
        .map(|(s, d, r, z)| Item {
            slug: s.to_string(),
            skill_dir: d.to_string(),
            repo: r.to_string(),
            zip: z.to_string(),
        })
        .collect()
}

/// O catálogo atual: tenta o índice remoto; cai no embutido se falhar/vazio.
pub fn catalog() -> Vec<Item> {
    if let Ok(body) = util::run(
        "curl",
        &["-sfL", "-m", "8", "-H", "User-Agent: schematize-cli", CATALOG_URL],
    ) {
        if let Ok(c) = serde_json::from_str::<Catalog>(&body) {
            if !c.skills.is_empty() {
                return c.skills;
            }
        }
    }
    builtin()
}

/// Resolve um slug dentro de um catálogo (clona o Item).
pub fn find(cat: &[Item], slug: &str) -> Option<Item> {
    cat.iter().find(|i| i.slug == slug).cloned()
}

/// URL do release "latest" (sempre a última versão publicada da skill).
pub fn latest_zip_url(it: &Item) -> String {
    format!(
        "https://github.com/{ORG}/{}/releases/latest/download/{}",
        it.repo, it.zip
    )
}
