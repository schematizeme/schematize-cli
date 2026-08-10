//! Registro das skills do catálogo schematize.
//! O quê: a lista canônica de skills instaláveis (org, repo, pasta, zip).
//! Onde: consultado por install/update/list. Futuras tools entram aqui.

/// Uma skill/tool instalável do ecossistema.
pub struct Item {
    /// slug curto (o que o usuário digita): "engineering", "go", ...
    pub slug: &'static str,
    /// nome da pasta instalada em ~/.claude/skills: "schematize-<slug>".
    pub skill_dir: &'static str,
    /// repositório GitHub: "skill-<slug>".
    pub repo: &'static str,
    /// nome do asset .zip do release.
    pub zip: &'static str,
}

/// Org GitHub que hospeda o catálogo.
pub const ORG: &str = "schematizeme";

/// Catálogo atual. Adicionar tool nova = uma linha aqui.
pub const ITEMS: &[Item] = &[
    Item { slug: "engineering", skill_dir: "schematize-engineering", repo: "skill-engineering", zip: "skill-engineering.zip" },
    Item { slug: "go",          skill_dir: "schematize-go",          repo: "skill-go",          zip: "skill-go.zip" },
    Item { slug: "rust",        skill_dir: "schematize-rust",        repo: "skill-rust",        zip: "skill-rust.zip" },
    Item { slug: "web",         skill_dir: "schematize-web",         repo: "skill-web",         zip: "skill-web.zip" },
    Item { slug: "node",        skill_dir: "schematize-node",        repo: "skill-node",        zip: "skill-node.zip" },
    Item { slug: "pentest",     skill_dir: "schematize-pentest",     repo: "skill-pentest",     zip: "skill-pentest.zip" },
];

/// Resolve um slug para o Item, ou None se desconhecido.
pub fn find(slug: &str) -> Option<&'static Item> {
    ITEMS.iter().find(|i| i.slug == slug)
}

/// URL do release "latest" (sempre a última versão publicada).
pub fn latest_zip_url(it: &Item) -> String {
    format!("https://github.com/{ORG}/{}/releases/latest/download/{}", it.repo, it.zip)
}
