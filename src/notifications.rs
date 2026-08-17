//! Notificações agregadas (o "sininho" da GUI) — SEM servidor por ora.
//! O quê: junta avisos de fontes LOCAIS em uma lista tipada: novidades GLOBAIS (nova versão do
//! app, posts recentes do blog) e avisos PESSOAIS (skills instaladas desatualizadas). Onde:
//! consumido pela GUI (badge do sininho + painel) e pelo CLI (`schematize notifications`).
//! Postura: TUDO é resiliente — falha de rede/skill é PULADA, nunca panica; `collect` sempre volta.
//!
//! As funções `notif_*` são PURAS (montam um `Notif` a partir de dados já colhidos) pra serem
//! testáveis sem rede; `collect` é a parte impura que faz as chamadas e delega a montagem a elas.

use crate::{news, registry, skills, upgrade};

/// Escopo de uma notificação: GLOBAL (vale pra todo mundo) ou PESSOAL (do ambiente do usuário).
pub enum NotifScope {
    /// Novidade geral: nova versão do app, post do blog.
    Global,
    /// Do ambiente do usuário: uma skill instalada dele está desatualizada.
    Personal,
}

/// Uma notificação já montada, pronta pra GUI/CLI renderizar.
pub struct Notif {
    /// Global ou Personal (agrupador na UI).
    pub scope: NotifScope,
    /// Título curto (1 linha).
    pub title: String,
    /// Corpo/resumo (1-2 linhas).
    pub body: String,
    /// Classe da notificação (pra ícone/ação na UI): "app_update" | "news" | "skill_outdated".
    pub kind: String,
    /// Ação sugerida: comando do CLI, palavra-chave ("upgrade") ou URL. `None` = só informativa.
    pub action: Option<String>,
}

/// Monta a notificação de "nova versão do app" (função pura, testável).
fn notif_app_update(cur: &str, new: &str) -> Notif {
    Notif {
        scope: NotifScope::Global,
        title: format!("Nova versão v{new}"),
        body: format!("O schematize v{new} está disponível (você está na v{cur})."),
        kind: "app_update".to_string(),
        action: Some("upgrade".to_string()),
    }
}

/// Monta a notificação de um post do blog (função pura, testável).
fn notif_post(title: &str, link: &str) -> Notif {
    Notif {
        scope: NotifScope::Global,
        title: title.to_string(),
        body: format!("Novo post no blog: {link}"),
        kind: "news".to_string(),
        action: Some(link.to_string()),
    }
}

/// Monta a notificação de "skill desatualizada" (função pura, testável).
fn notif_skill_outdated(slug: &str, installed: &str, latest: &str) -> Notif {
    Notif {
        scope: NotifScope::Personal,
        title: format!("Skill {slug} desatualizada"),
        body: format!("v{installed} → v{latest}"),
        kind: "skill_outdated".to_string(),
        action: Some(format!("skills update {slug}")),
    }
}

/// Colhe TODAS as notificações locais (global + pessoal). Resiliente: cada fonte que falha
/// (rede, skill ausente) é simplesmente pulada — nunca panica e sempre retorna uma lista.
/// Custo de rede: resolve a versão de cada skill INSTALADA (a GUI chama isto numa thread).
pub fn collect() -> Vec<Notif> {
    let mut out: Vec<Notif> = Vec::new();

    // --- GLOBAL (a) nova versão do app.
    if let Some((cur, new)) = upgrade::app_update_available() {
        out.push(notif_app_update(&cur, &new));
    }

    // --- GLOBAL (b) posts recentes do blog (best-effort; vazio se sem feed).
    for post in news::latest(5) {
        out.push(notif_post(&post.title, &post.link));
    }

    // --- PESSOAL: skills instaladas do usuário que estão desatualizadas.
    // Cruza o estado (o que ele tem) com o catálogo (pra saber onde checar a última).
    let st = skills::load_state();
    let cat = registry::catalog();
    for slug in st.skills.keys() {
        let Some(it) = registry::find(&cat, slug) else { continue }; // fora do catálogo → pula
        let Some(installed) = skills::installed_version(&it) else { continue }; // não está em disco
        // Skill forkada não "desatualiza" no sentido normal — a via é comparar/mesclar, não sobrescrever.
        if st.skills.get(slug).map(|e| e.forked).unwrap_or(false) {
            continue;
        }
        let Ok(latest) = skills::resolve_latest(&it) else { continue }; // rede falhou → pula
        if crate::util::semver_lt(&installed, &latest) {
            out.push(notif_skill_outdated(slug, &installed, &latest));
        }
    }

    out
}

/// Quantidade de notificações (pro badge do sininho). É `collect().len()`.
pub fn count() -> usize {
    collect().len()
}

// ------------------------------------------------------------------------------------------------
// Testes: montagem a partir de dados sintéticos (funções puras) — sem rede.
// ------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monta_notif_app_update() {
        let n = notif_app_update("0.25.0", "0.26.0");
        assert!(matches!(n.scope, NotifScope::Global));
        assert_eq!(n.kind, "app_update");
        assert_eq!(n.action.as_deref(), Some("upgrade"));
        assert!(n.title.contains("0.26.0"));
        assert!(n.body.contains("0.25.0"));
    }

    #[test]
    fn monta_notif_post() {
        let n = notif_post("Título do post", "https://blog.schematize.net/x");
        assert!(matches!(n.scope, NotifScope::Global));
        assert_eq!(n.kind, "news");
        assert_eq!(n.action.as_deref(), Some("https://blog.schematize.net/x"));
        assert_eq!(n.title, "Título do post");
    }

    #[test]
    fn monta_notif_skill_outdated() {
        let n = notif_skill_outdated("rust", "0.14.0", "0.15.0");
        assert!(matches!(n.scope, NotifScope::Personal));
        assert_eq!(n.kind, "skill_outdated");
        assert_eq!(n.action.as_deref(), Some("skills update rust"));
        assert!(n.title.contains("rust"));
        assert!(n.body.contains("0.14.0") && n.body.contains("0.15.0"));
    }

    #[test]
    fn count_bate_com_collect() {
        // `collect`/`count` não podem panicar mesmo sem rede — só checamos a igualdade
        // (o número real depende do ambiente/rede, mas a chamada tem que ser resiliente).
        assert_eq!(count(), collect().len());
    }
}
