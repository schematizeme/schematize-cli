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

/// Autor/co-autor da skill (mostrado na GUI): a pessoa/organização por trás (credibilidade).
/// Só nome + link — quem quiser verificar a firma vai ao site do parceiro (o `url`). Sem doc.
#[derive(Clone, Deserialize)]
pub struct Sponsor {
    pub name: String,
    pub url: String,
}

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
    /// grupo na GUI: "base" (arquitetura), "language" (linguagem), "external" (ferramenta).
    #[serde(default)]
    pub category: String,
    /// autor/co-autor (pessoa/org) por trás da skill.
    #[serde(default)]
    pub sponsor: Option<Sponsor>,
    /// selo de VERIFICADO — skill oficial/de parceiro vetado (vs. futura skill de comunidade).
    #[serde(default)]
    pub verified: bool,
    /// slugs de skills COMPLEMENTARES recomendadas — ex.: toda linguagem recomenda a
    /// "engineering" (a skill BASE). Consumido pelo install (sugere/instala com --with-recommended)
    /// e exposto pra GUI ler depois. `#[serde(default)]` = catálogo antigo (sem o campo) ainda parseia.
    #[serde(default)]
    pub recommends: Vec<String>,
}

#[derive(Deserialize)]
struct Catalog {
    skills: Vec<Item>,
}

/// Catálogo embutido — fallback quando o índice remoto está indisponível (offline).
/// Mantido em dia como rede de segurança; a fonte de verdade é o `catalog.json`.
fn builtin() -> Vec<Item> {
    let sp = |name: &str, url: &str| Some(Sponsor { name: name.into(), url: url.into() });
    let me = || sp("Lucassa", "https://lucassa.me");
    // Todas as skills oficiais nascem verificadas (publisher conhecido/vetado).
    let mk = |slug: &str, cat: &str, sponsor: Option<Sponsor>, recommends: &[&str]| Item {
        slug: slug.into(),
        skill_dir: format!("schematize-{slug}"),
        repo: format!("skill-{slug}"),
        zip: format!("skill-{slug}.zip"),
        category: cat.into(),
        sponsor,
        verified: true,
        recommends: recommends.iter().map(|s| s.to_string()).collect(),
    };
    // Toda skill de LINGUAGEM recomenda a "engineering" (a skill BASE complementar).
    let lang = ["engineering"];
    vec![
        mk("engineering", "base", me(), &[]),
        mk("web", "base", me(), &[]),
        mk("go", "language", me(), &lang),
        mk("rust", "language", me(), &lang),
        mk("elixir", "language", me(), &lang),
        mk("csharp", "language", me(), &lang),
        mk("zig", "language", me(), &lang),
        mk("ruby", "language", me(), &lang),
        mk("node", "language", me(), &lang),
        mk("seo", "external", sp("Hextorn", "https://hextorn.com"), &[]),
        mk("pentest", "external", sp("Basilisk Offsec", "https://basiliskoffsec.com"), &[]),
        mk("audit", "external", me(), &[]),
        mk("ai", "base", me(), &[]),
        mk("data", "base", me(), &[]),
        mk("mobile", "base", me(), &[]),
        mk("infra", "base", me(), &[]),
        mk("docs", "base", me(), &[]),
        mk("scaffold", "base", me(), &[]),
        mk("overdev-context", "base", me(), &["engineering"]),
    ]
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
