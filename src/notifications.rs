//! Notificações agregadas (o "sininho" da GUI) — SEM servidor por ora.
//! O quê: junta avisos de fontes LOCAIS em uma lista tipada: novidades GLOBAIS (nova versão do
//! app, posts recentes do blog) e avisos PESSOAIS (skills instaladas desatualizadas). Onde:
//! consumido pela GUI (badge do sininho + painel) e pelo CLI (`schematize notifications`).
//! Postura: TUDO é resiliente — falha de rede/skill é PULADA, nunca panica; `collect` sempre volta.
//!
//! As funções `notif_*` são PURAS (montam um `Notif` a partir de dados já colhidos) pra serem
//! testáveis sem rede; `collect` é a parte impura que faz as chamadas e delega a montagem a elas.

use crate::{account, news, registry, skills, upgrade, util};
use serde::Deserialize;

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

/// Forma bruta de uma notificação do servidor (marketplace). Campos ausentes viram default —
/// catálogo/servidor antigo ainda parseia. `object` (url/id do alvo) vira a `action` da UI.
#[derive(Deserialize)]
struct ServerNotif {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    object: Option<String>,
}

/// Página de `GET /me/notifications` (só o `data` interessa aqui).
#[derive(Deserialize)]
struct NotifPage {
    #[serde(default)]
    data: Vec<ServerNotif>,
}

/// Faz o parse PURO da página de notificações do servidor em `Notif` (escopo Pessoal).
/// Testável sem rede. JSON inválido → lista vazia (best-effort, nunca panica).
fn parse_notifs(json: &str) -> Vec<Notif> {
    let page: NotifPage = match serde_json::from_str(json) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    page.data
        .into_iter()
        .map(|s| Notif {
            scope: NotifScope::Personal,
            title: s.title,
            body: s.body,
            kind: if s.kind.trim().is_empty() {
                "server".to_string()
            } else {
                s.kind
            },
            action: s.object.filter(|o| !o.trim().is_empty()),
        })
        .collect()
}

/// Busca as notificações do SERVIDOR (marketplace), se o usuário estiver logado. Best-effort:
/// não logado, sem access token válido, ou falha de rede → lista vazia (só as locais aparecem).
/// A parte impura (rede) fica aqui; a montagem delega a `parse_notifs` (pura, testável).
fn server_notifs() -> Vec<Notif> {
    if !account::is_logged_in() {
        return Vec::new();
    }
    let Some(token) = account::access_token() else {
        return Vec::new();
    };
    let url = format!("{}/me/notifications?unread=false&limit=20", account::api_base());
    let auth = format!("Authorization: Bearer {token}");
    match util::run(
        "curl",
        &["-sfL", "-m", "10", "-H", "User-Agent: schematize-cli", "-H", &auth, &url],
    ) {
        Ok(body) => parse_notifs(&body),
        Err(_) => Vec::new(),
    }
}

/// Colhe TODAS as notificações (locais global+pessoal + do servidor, se logado). Resiliente:
/// cada fonte que falha (rede, skill ausente, não-logado) é simplesmente pulada — nunca panica
/// e sempre retorna uma lista. Custo de rede: versão de cada skill INSTALADA + (se logado) o
/// GET das notificações do servidor (a GUI chama isto numa thread).
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
    // Candidatos: skill instalada, no catálogo, NÃO forkada (fork compara/mescla, não desatualiza).
    let cands: Vec<(String, registry::Item, String)> = st
        .skills
        .keys()
        .filter_map(|slug| {
            let it = registry::find(&cat, slug)?;
            let installed = skills::installed_version(&it)?;
            if st.skills.get(slug).map(|e| e.forked).unwrap_or(false) {
                return None;
            }
            Some((slug.clone(), it, installed))
        })
        .collect();
    // PARALELIZA o `resolve_latest` de cada skill — era um N+1 SEQUENCIAL (1 ida ao GitHub por skill,
    // uma após a outra) e o principal motivo do sininho ser lento. Agora tudo concorrente (1 thread
    // por skill, curl bloqueante em cada). ~20 idas em série viram ~1 latência.
    let handles: Vec<_> = cands
        .into_iter()
        .map(|(slug, it, installed)| {
            std::thread::spawn(move || match skills::resolve_latest(&it) {
                Ok(latest) if crate::util::semver_lt(&installed, &latest) => {
                    Some((slug, installed, latest))
                }
                _ => None,
            })
        })
        .collect();
    for h in handles {
        if let Ok(Some((slug, installed, latest))) = h.join() {
            out.push(notif_skill_outdated(&slug, &installed, &latest));
        }
    }

    // --- SERVIDOR: notificações do marketplace (só se logado; best-effort).
    out.extend(server_notifs());

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
    fn parse_notifs_do_servidor() {
        let j = r#"{
            "data":[
                {"id":"1","kind":"review_reply","title":"Alguém respondeu","body":"na sua review","object":"https://x/r/1","read":false,"created_at":"2026-08-16T00:00:00Z"},
                {"id":"2","kind":"","title":"Sem kind","body":"","object":null,"read":true}
            ],
            "next_before":"2026-08-15T00:00:00Z"
        }"#;
        let ns = parse_notifs(j);
        assert_eq!(ns.len(), 2);
        assert!(matches!(ns[0].scope, NotifScope::Personal));
        assert_eq!(ns[0].kind, "review_reply");
        assert_eq!(ns[0].title, "Alguém respondeu");
        assert_eq!(ns[0].action.as_deref(), Some("https://x/r/1"));
        // kind vazio vira "server"; object null/ausente → action None.
        assert_eq!(ns[1].kind, "server");
        assert_eq!(ns[1].action, None);
    }

    #[test]
    fn parse_notifs_invalido_vira_vazio() {
        assert!(parse_notifs("não json").is_empty());
        assert!(parse_notifs("{}").is_empty());
        assert!(parse_notifs(r#"{"data":[]}"#).is_empty());
    }

    #[test]
    fn count_bate_com_collect() {
        // `collect`/`count` não podem panicar mesmo sem rede — só checamos a igualdade
        // (o número real depende do ambiente/rede, mas a chamada tem que ser resiliente).
        assert_eq!(count(), collect().len());
    }
}
