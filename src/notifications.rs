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
    use crate::notificacoes::formato::{acao_valida, texto_limpo, Kind, Origem, MAX_CORPO, MAX_ITENS, MAX_TITULO};
    let page: NotifPage = match serde_json::from_str(json) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    page.data
        .into_iter()
        // Teto de itens ANTES de processar: uma resposta com 100 mil entradas não pode
        // virar 100 mil alocações só pra ser descartada depois.
        .take(MAX_ITENS)
        .filter_map(|s| {
            // Origem REMOTA: o servidor não pode declarar um tipo que aciona o app, nem
            // produzir ação interna. Campo que não casa DERRUBA a notificação inteira —
            // remendar dado hostil é adivinhar a intenção de quem talvez seja o atacante.
            let kind = Kind::parse(&s.kind, Origem::Remota)?;
            let title = texto_limpo(&s.title, MAX_TITULO)?;
            let body = texto_limpo(&s.body, MAX_CORPO).unwrap_or_default();
            let acao = acao_valida(s.object.as_deref(), &kind, Origem::Remota);
            Some(Notif {
                scope: NotifScope::Personal,
                title,
                body,
                kind: kind.como_str().to_string(),
                action: match acao.como_str() {
                    "" => None,
                    a => Some(a.to_string()),
                },
            })
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
    // O feed do blog é REDE: título e link passam pela mesma fronteira do servidor.
    // Um feed envenenado (ou interceptado) mandaria ANSI no título e `file://` no link,
    // e o link vai direto pro navegador do usuário.
    {
        use crate::notificacoes::formato::{texto_limpo, url_segura, MAX_TITULO};
        for post in news::latest(5) {
            let (Some(t), Some(u)) = (texto_limpo(&post.title, MAX_TITULO), url_segura(&post.link)) else {
                continue;
            };
            out.push(notif_post(&t, &u));
        }
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
    // AGREGADOR primeiro: UMA chamada a `{api}/versions?skills=...` traz todas as versões — a
    // mesma fonte que o site lê (o espelho que o `sync-skills` alimenta a partir dos releases).
    // Era um N+1 (1 ida ao GitHub por skill); virou 1 request.
    let slugs: Vec<String> = cands.iter().map(|(s, _, _)| s.clone()).collect();
    let bulk = skills::latest_versions_bulk(&slugs);

    // O que o agregador respondeu resolve na hora; o resto (API fora, skill nova que a API ainda
    // não conhece) cai no caminho antigo — raw do GitHub, uma thread por skill, concorrente.
    let mut fallback: Vec<(String, registry::Item, String)> = Vec::new();
    for (slug, it, installed) in cands {
        match bulk.get(&slug) {
            Some(latest) => {
                if crate::util::semver_lt(&installed, latest) {
                    out.push(notif_skill_outdated(&slug, &installed, latest));
                }
            }
            None => fallback.push((slug, it, installed)),
        }
    }
    let handles: Vec<_> = fallback
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

/// Quantidade pro badge — do CACHE LOCAL, sem rede.
///
/// Era `collect().len()`, ou seja: o badge fazia a rodada de rede inteira e, ao abrir o
/// painel, ela era refeita. Duas idas independentes pra a mesma pergunta, e um timer
/// repetindo a cada 90s. Quando a segunda falhava, o badge dizia "3" e o painel vinha
/// vazio — o "marca e não carrega". Agora badge e painel leem a MESMA fonte.
pub fn count() -> usize {
    crate::notificacoes::cache::nao_lidas(&crate::notificacoes::cache::ler())
}

/// Coleta da rede e FUNDE no cache local. É a única função que fala com a rede.
///
/// Devolve quantas ficaram não-lidas. Chamada em thread; se a rede falhar, o cache
/// permanece — "não chega nada novo" em vez de "some tudo".
pub fn sincronizar() -> usize {
    use crate::notificacoes::cache;
    use crate::notificacoes::formato::{acao_valida, texto_limpo, Kind, Origem, MAX_CORPO, MAX_TITULO};
    let colhidas = collect();
    let novas: Vec<cache::Registro> = colhidas
        .into_iter()
        .filter_map(|n| {
            // Segunda passada de sanitização, agora na origem LOCAL. Parece redundante
            // com o que já foi validado na borda remota, e é de propósito: garante que
            // NADA chega ao cache sem passar pelo formato fechado, inclusive o que este
            // binário montou. Uma fonte nova que alguém acrescente amanhã já nasce coberta.
            let kind = Kind::parse(&n.kind, Origem::Local)?;
            let titulo = texto_limpo(&n.title, MAX_TITULO)?;
            let corpo = texto_limpo(&n.body, MAX_CORPO).unwrap_or_default();
            let acao = acao_valida(n.action.as_deref(), &kind, Origem::Local);
            let escopo = match n.scope {
                NotifScope::Global => "global",
                NotifScope::Personal => "personal",
            };
            Some(cache::novo(escopo, &kind, titulo, corpo, &acao))
        })
        .collect();
    let fundido = cache::fundir(&cache::ler(), novas);
    let n = cache::nao_lidas(&fundido);
    let _ = cache::gravar(&fundido);
    n
}

/// O que a UI mostra: o cache inteiro (inclusive o histórico), sem tocar a rede.
pub fn listar() -> Vec<crate::notificacoes::cache::Registro> {
    crate::notificacoes::cache::ler()
}

/// Marca tudo que está NOVO como lido (o painel foi aberto). Devolve quantas mudaram.
pub fn marcar_lidas() -> usize {
    use crate::notificacoes::cache;
    let mut v = cache::ler();
    let n = cache::marcar_todas_lidas(&mut v);
    if n > 0 {
        let _ = cache::gravar(&v);
    }
    n
}

/// Marca uma como CONCLUÍDA (a ação foi tomada). Não apaga: vai pro histórico.
pub fn concluir(id: &str) -> bool {
    use crate::notificacoes::cache;
    let mut v = cache::ler();
    if cache::marcar(&mut v, id, cache::Estado::Concluida) {
        let _ = cache::gravar(&v);
        return true;
    }
    false
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

    /// O QUE: o badge (`count`) sai do CACHE, nunca de uma rodada de rede; e nem ele nem
    /// `collect` panicam sem rede.
    ///
    /// POR QUE assim, e não `count() == collect().len()` (o que este teste assertava):
    /// aquela igualdade era o comportamento ANTERIOR à v0.50.0, quando `count()` era
    /// literalmente `collect().len()` — e era o BUG: o badge fazia a rodada de rede
    /// inteira, o painel fazia OUTRA, e quando a segunda falhava o badge dizia "3" com o
    /// painel vazio. A v0.50.0 desacoplou de propósito (o cache virou a única fonte da UI),
    /// então a igualdade deixou de valer. O teste só continuava verde por COINCIDÊNCIA de
    /// ambiente: com cache vazio e rede vazia, `0 == 0`. Assim que o cache encheu (23) e a
    /// rede devolveu 0 — máquina sem sessão — ele ficou vermelho, acusando o conserto em
    /// vez do defeito. Uma asserção que depende do estado da máquina não é guarda: é sorte.
    ///
    /// A asserção abaixo é o contrato que a v0.50.0 criou. Se alguém voltar `count()` a
    /// consultar a rede, ela quebra — que é exatamente a regressão a impedir.
    #[test]
    fn badge_le_o_cache_e_nao_a_rede() {
        use crate::notificacoes::cache;
        assert_eq!(
            count(),
            cache::nao_lidas(&cache::ler()),
            "o badge tem que refletir o cache — se voltar a somar o resultado da rede, \
             o painel e o número divergem de novo (o bug da v0.50.0)"
        );
        // Resiliência: sem rede, `collect` degrada pra lista (possivelmente vazia), não panica.
        let _ = collect();
    }

    /// PAYLOAD HOSTIL ponta a ponta: um servidor comprometido tentando (a) disparar o
    /// auto-update do cliente, (b) mandar o navegador abrir um `file://`, (c) forjar a
    /// saída do terminal com ANSI, e (d) estourar a memória com um corpo gigante.
    ///
    /// Este teste existe porque as regras individuais já têm teste em `formato`, mas o
    /// que importa é a COMPOSIÇÃO: é fácil validar tudo e ainda assim deixar o dado
    /// hostil passar por um caminho que ninguém ligou.
    #[test]
    fn payload_hostil_do_servidor_nao_passa() {
        let corpo_gigante = "x".repeat(50_000);
        let j = format!(
            r#"{{"data":[
                {{"kind":"app_update","title":"Atualize agora","body":"","object":"upgrade"}},
                {{"kind":"news","title":"Post","body":"","object":"file:///etc/passwd"}},
                {{"kind":"news","title":"\u001b[2J FALSO tudo certo","body":"{corpo_gigante}","object":"https://blog.schematize.org/p"}},
                {{"kind":"skill_outdated","title":"x","body":"","object":"skills update a; rm -rf /"}},
                {{"kind":"review_reply","title":"Alguém respondeu","body":"na sua review","object":"https://app.schematize.org/r/1"}}
            ]}}"#
        );
        let ns = parse_notifs(&j);

        // (a) escalada de tipo: as duas que ACIONAM o app sumiram inteiras.
        assert!(!ns.iter().any(|n| n.kind == "app_update"), "servidor não dispara auto-update");
        assert!(!ns.iter().any(|n| n.kind == "skill_outdated"));

        // (b) `file://` não vira ação — a notificação sobrevive, o gatilho não.
        let post = ns.iter().find(|n| n.title == "Post").expect("o post em si é legítimo");
        assert_eq!(post.action, None, "URI não-https não chega ao xdg-open");

        // (c) ANSI removido e corpo cortado no teto.
        let forjada = ns.iter().find(|n| n.title.contains("FALSO")).unwrap();
        assert!(!forjada.title.contains('\u{1b}'), "escape sobreviveu: {:?}", forjada.title);
        assert!(forjada.body.chars().count() <= crate::notificacoes::formato::MAX_CORPO);
        assert_eq!(forjada.action.as_deref(), Some("https://blog.schematize.org/p"), "https legítimo passa");

        // (d) o tipo legítimo do servidor continua funcionando — sanitizar não é quebrar.
        let r = ns.iter().find(|n| n.kind == "review_reply").expect("tipo legítimo preservado");
        assert_eq!(r.action.as_deref(), Some("https://app.schematize.org/r/1"));
    }

    /// Resposta gigantesca é cortada ANTES de virar alocação.
    #[test]
    fn resposta_gigante_respeita_o_teto_de_itens() {
        let itens: Vec<String> = (0..5_000)
            .map(|i| format!(r#"{{"kind":"news","title":"t{i}","body":"","object":null}}"#))
            .collect();
        let j = format!(r#"{{"data":[{}]}}"#, itens.join(","));
        assert_eq!(parse_notifs(&j).len(), crate::notificacoes::formato::MAX_ITENS);
    }
}
