//! market — notas (ratings) do marketplace refletidas no app.
//! O quê: lê a média + contagem de avaliações de uma skill (ou de TODAS de uma vez) da API
//! pública do marketplace (`schematizeskills_api_rs`), pra CLI/GUI mostrarem a nota nas linhas.
//! Onde: consumido por `skills list` (CLI) e pela GUI (uma request pra todas as linhas).
//!
//! Rede: padrão da casa (curl por `util::run`, sem crate de HTTP). Endpoints PÚBLICOS (sem auth).
//! Postura: TUDO é resiliente — falha de rede/parse vira None/mapa vazio, nunca panica nem trava a UI.
//! As funções de parsing são PURAS (testáveis sem rede).

use crate::account;
use crate::util;
use std::collections::HashMap;

/// Timeout curto (s) do curl — a nota é enfeite; nunca deve travar a UI se a API estiver lenta/off.
const NET_TIMEOUT: &str = "6";

/// Nota de UMA skill: `(média, contagem)` via `GET {api}/skills/{slug}` (público). None em
/// erro/rede/parse ou se a resposta não traz os campos de avaliação. Nunca panica.
pub fn market_rating(slug: &str) -> Option<(f32, u32)> {
    let url = format!("{}/skills/{}", account::api_base(), slug);
    let body =
        util::run("curl", &["-sfL", "-m", NET_TIMEOUT, "-H", "User-Agent: schematize-cli", &url])
            .ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    // A skill pode vir na raiz ou aninhada em "skill"/"data".
    let node = v.get("skill").or_else(|| v.get("data")).unwrap_or(&v);
    extract_rating(node)
}

/// Nota de TODAS as skills num único request: `GET {api}/skills` (público). Mapeia
/// `slug -> (média, contagem)` só para as que trazem avaliação. Mapa vazio em erro/rede.
/// Feito pra a GUI pintar a nota em toda linha com UMA chamada. Nunca panica.
pub fn market_ratings_all() -> HashMap<String, (f32, u32)> {
    let url = format!("{}/skills", account::api_base());
    let body = match util::run(
        "curl",
        &["-sfL", "-m", NET_TIMEOUT, "-H", "User-Agent: schematize-cli", &url],
    ) {
        Ok(b) => b,
        Err(_) => return HashMap::new(),
    };
    parse_ratings_all(&body)
}

// ------------------------------------------------------------------------------------------------
// Parsing PURO (testável sem rede).
// ------------------------------------------------------------------------------------------------

/// Extrai `(média, contagem)` de um nó JSON de skill, tolerante a nomes de campo diferentes
/// (a API pode chamar de `rating_avg`/`average`/`rating`/`stars`… e `rating_count`/`reviews`…).
/// Se não achar a MÉDIA, devolve None (sem média não há nota a mostrar); contagem ausente = 0.
pub fn extract_rating(node: &serde_json::Value) -> Option<(f32, u32)> {
    let avg = first_f32(
        node,
        &[
            "rating_avg",
            "rating_average",
            "average_rating",
            "average",
            "avg",
            "rating",
            "stars",
            "score",
        ],
    )?;
    let count = first_u32(
        node,
        &[
            "rating_count",
            "ratings_count",
            "reviews_count",
            "review_count",
            "num_ratings",
            "count",
            "reviews",
            "ratings",
        ],
    )
    .unwrap_or(0);
    // Sanidade: nota fora de [0,5] é dado ruim → descarta (deny-by-default).
    if !(0.0..=5.0).contains(&avg) {
        return None;
    }
    Some((avg, count))
}

/// Faz o parse do `GET /skills` (lista) em `slug -> (média, contagem)`. Aceita tanto um array
/// puro `[...]` quanto um envelope `{skills:[...]}`/`{data:[...]}`. Ignora itens sem slug/sem nota.
pub fn parse_ratings_all(body: &str) -> HashMap<String, (f32, u32)> {
    let mut out = HashMap::new();
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let arr = if v.is_array() {
        v.as_array().cloned().unwrap_or_default()
    } else {
        v.get("skills")
            .or_else(|| v.get("data"))
            .or_else(|| v.get("items"))
            .and_then(|x| x.as_array().cloned())
            .unwrap_or_default()
    };
    for item in arr {
        let slug = item.get("slug").or_else(|| item.get("name")).and_then(|s| s.as_str());
        if let (Some(slug), Some(rating)) = (slug, extract_rating(&item)) {
            out.insert(slug.to_string(), rating);
        }
    }
    out
}

/// Primeiro campo (dentre `keys`) que der pra ler como f32 (aceita número OU string numérica).
fn first_f32(node: &serde_json::Value, keys: &[&str]) -> Option<f32> {
    for k in keys {
        if let Some(val) = node.get(*k) {
            if let Some(n) = val.as_f64() {
                return Some(n as f32);
            }
            if let Some(s) = val.as_str() {
                if let Ok(n) = s.trim().parse::<f32>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Primeiro campo (dentre `keys`) que der pra ler como u32 (número OU string numérica).
fn first_u32(node: &serde_json::Value, keys: &[&str]) -> Option<u32> {
    for k in keys {
        if let Some(val) = node.get(*k) {
            if let Some(n) = val.as_u64() {
                return Some(n as u32);
            }
            if let Some(n) = val.as_f64() {
                return Some(n as u32);
            }
            if let Some(s) = val.as_str() {
                if let Ok(n) = s.trim().parse::<u32>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Formata a nota pra UI compacta: `★4.7 (12)`. Vazio se não houver nota.
pub fn format_rating(rating: Option<(f32, u32)>) -> String {
    match rating {
        Some((avg, count)) => format!("★{avg:.1} ({count})"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrai_media_e_contagem_de_nomes_variados() {
        let a = serde_json::json!({ "rating_avg": 4.6, "rating_count": 21 });
        assert_eq!(extract_rating(&a), Some((4.6, 21)));
        // Nomes alternativos + contagem ausente = 0.
        let b = serde_json::json!({ "average": 3.5 });
        assert_eq!(extract_rating(&b), Some((3.5, 0)));
        // Strings numéricas também servem.
        let c = serde_json::json!({ "rating": "5", "reviews": "8" });
        assert_eq!(extract_rating(&c), Some((5.0, 8)));
        // Sem média → None.
        let d = serde_json::json!({ "reviews": 3 });
        assert_eq!(extract_rating(&d), None);
        // Média fora de faixa → descartada.
        let e = serde_json::json!({ "rating": 9.9 });
        assert_eq!(extract_rating(&e), None);
    }

    #[test]
    fn lista_array_puro_mapeia_slug_para_nota() {
        let body = r#"[
            {"slug":"engineering","rating_avg":4.8,"rating_count":30},
            {"slug":"seo","average":4.2,"reviews":5},
            {"slug":"sem-nota"}
        ]"#;
        let m = parse_ratings_all(body);
        assert_eq!(m.get("engineering"), Some(&(4.8, 30)));
        assert_eq!(m.get("seo"), Some(&(4.2, 5)));
        assert!(!m.contains_key("sem-nota")); // sem média não entra
    }

    #[test]
    fn lista_com_envelope_skills() {
        let body = r#"{"skills":[{"slug":"rust","rating":4.9,"rating_count":12}]}"#;
        let m = parse_ratings_all(body);
        assert_eq!(m.get("rust"), Some(&(4.9, 12)));
    }

    #[test]
    fn lista_invalida_vira_mapa_vazio() {
        assert!(parse_ratings_all("não é json").is_empty());
        assert!(parse_ratings_all("{}").is_empty());
    }

    #[test]
    fn format_rating_compacto() {
        assert_eq!(format_rating(Some((4.66, 12))), "★4.7 (12)");
        assert_eq!(format_rating(None), "");
    }
}
