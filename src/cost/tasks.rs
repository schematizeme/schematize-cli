//! Atribuição de task: (1) tag explícita (`cost tag`) → (2) item ativo do checklist do overdev
//! → (3) `sem-task`. O registro das tags é `tasks.jsonl`, append-only.

use super::tempo::{fmt_ts, parse_ts};
use super::SEM_TASK;
use crate::paths;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

/// Item do overdev só vale pra respostas dos últimos instantes: ele é o item ativo AGORA, e
/// aplicá-lo a um backfill de semanas atribuiria o passado ao item errado.
const JANELA_OVERDEV_H: i64 = 24;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evento {
    pub ts: String,
    pub task_id: String,
    #[serde(default)]
    pub nome: Option<String>,
    /// `start` | `end`.
    pub acao: String,
}

/// Eventos em ordem cronológica. Linha corrompida é pulada (o registro é de humano e de hook).
pub fn ler_eventos(path: &Path) -> Vec<Evento> {
    let txt = std::fs::read_to_string(path).unwrap_or_default();
    let mut v: Vec<(DateTime<Utc>, Evento)> = txt
        .lines()
        .filter_map(|l| serde_json::from_str::<Evento>(l).ok())
        .filter_map(|e| parse_ts(&e.ts).map(|t| (t, e)))
        .collect();
    v.sort_by_key(|(t, _)| *t);
    v.into_iter().map(|(_, e)| e).collect()
}

/// Anexa um evento (cria o arquivo).
pub fn registrar(
    path: &Path,
    agora: DateTime<Utc>,
    task_id: &str,
    nome: Option<&str>,
    acao: &str,
) -> Result<(), String> {
    let e = Evento {
        ts: fmt_ts(agora),
        task_id: task_id.to_string(),
        nome: nome.map(str::to_string),
        acao: acao.to_string(),
    };
    let mut l = serde_json::to_string(&e).map_err(|e| format!("evento: {e}"))?;
    l.push('\n');
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("abrir {}: {e}", path.display()))?;
    f.write_all(l.as_bytes()).map_err(|e| format!("gravar {}: {e}", path.display()))
}

/// A tag em vigor em `ts`: o último evento ≤ `ts`, se for `start`.
pub fn tag_em(eventos: &[Evento], ts: DateTime<Utc>) -> Option<&Evento> {
    let ult = eventos.iter().rfind(|e| parse_ts(&e.ts).is_some_and(|t| t <= ts))?;
    (ult.acao == "start").then_some(ult)
}

/// A tag aberta agora (último evento é `start`).
pub fn tag_aberta(eventos: &[Evento]) -> Option<&Evento> {
    eventos.last().filter(|e| e.acao == "start")
}

/// Nome legível de cada task (do último `start` que o trouxe).
pub fn nomes(eventos: &[Evento]) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    for e in eventos.iter().filter(|e| e.acao == "start") {
        if let Some(n) = e.nome.as_ref().filter(|n| !n.is_empty()) {
            m.insert(e.task_id.clone(), n.clone());
        }
    }
    m
}

/// O primeiro item ABERTO (`- [ ]`) do CHECKLIST do overdev → `overdev:<id>`.
///
/// `[H ]` (humano) e `[~]` (on-hold) não são trabalho da máquina agora, então não contam. O id é
/// o primeiro token se parece um código (`A8`, `3.2`); senão, um slug do começo do texto.
pub fn item_ativo_overdev(root: &Path) -> Option<String> {
    let od = paths::overdev_dir_at(root);
    let txt = paths::read_multidoc(&od, "CHECKLIST.md", "checklist");
    txt.lines().find_map(|l| {
        let t = l.trim_start();
        let resto = t.strip_prefix("- [ ]").or_else(|| t.strip_prefix("* [ ]"))?;
        id_do_item(resto)
    })
}

fn id_do_item(texto: &str) -> Option<String> {
    let limpo: String = texto.chars().filter(|c| *c != '*' && *c != '`').collect();
    let limpo = limpo.trim();
    let tok = limpo.split_whitespace().next()?.trim_end_matches([':', '.', '—', '-', ')']);
    if !tok.is_empty() && tok.len() <= 16 && tok.chars().any(|c| c.is_ascii_digit()) {
        return Some(format!("overdev:{tok}"));
    }
    let slug: String = limpo
        .chars()
        .take(40)
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    (!slug.is_empty()).then(|| format!("overdev:{slug}"))
}

/// A task de uma resposta: tag → overdev (se recente) → `sem-task`.
pub fn resolver(
    ts: DateTime<Utc>,
    eventos: &[Evento],
    overdev: Option<&str>,
    agora: DateTime<Utc>,
) -> String {
    if let Some(e) = tag_em(eventos, ts) {
        return e.task_id.clone();
    }
    match overdev {
        Some(id) if ts >= agora - Duration::hours(JANELA_OVERDEV_H) => id.to_string(),
        _ => SEM_TASK.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        parse_ts(s).unwrap()
    }

    fn ev(ts: &str, id: &str, acao: &str) -> Evento {
        Evento { ts: ts.into(), task_id: id.into(), nome: None, acao: acao.into() }
    }

    #[test]
    fn tag_vale_do_start_ate_o_end_ou_o_proximo_start() {
        let e = vec![
            ev("2026-10-31T10:00:00Z", "A", "start"),
            ev("2026-10-31T11:00:00Z", "A", "end"),
            ev("2026-10-31T12:00:00Z", "B", "start"),
            ev("2026-10-31T13:00:00Z", "C", "start"),
        ];
        assert!(tag_em(&e, t("2026-10-31T09:59:00Z")).is_none());
        assert_eq!(tag_em(&e, t("2026-10-31T10:30:00Z")).unwrap().task_id, "A");
        assert!(tag_em(&e, t("2026-10-31T11:30:00Z")).is_none());
        assert_eq!(tag_em(&e, t("2026-10-31T12:59:00Z")).unwrap().task_id, "B");
        assert_eq!(tag_em(&e, t("2026-10-31T23:00:00Z")).unwrap().task_id, "C");
        assert_eq!(tag_aberta(&e).unwrap().task_id, "C");
    }

    #[test]
    fn ordem_de_atribuicao_tag_depois_overdev_depois_sem_task() {
        let agora = t("2026-10-31T12:00:00Z");
        let e = vec![
            ev("2026-10-31T10:00:00Z", "T1", "start"),
            ev("2026-10-31T10:30:00Z", "T1", "end"),
        ];
        // dentro da tag
        assert_eq!(resolver(t("2026-10-31T10:10:00Z"), &e, Some("overdev:A8"), agora), "T1");
        // fora da tag, recente → overdev
        assert_eq!(
            resolver(t("2026-10-31T11:00:00Z"), &e, Some("overdev:A8"), agora),
            "overdev:A8"
        );
        // sem overdev → sem-task
        assert_eq!(resolver(t("2026-10-31T11:00:00Z"), &e, None, agora), SEM_TASK);
        // backfill antigo não herda o item ativo de hoje
        assert_eq!(resolver(t("2026-10-01T11:00:00Z"), &e, Some("overdev:A8"), agora), SEM_TASK);
    }

    #[test]
    fn registro_roundtrip_e_linha_ruim_ignorada() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("tasks.jsonl");
        registrar(&p, t("2026-10-31T10:00:00Z"), "X1", Some("Fazer X"), "start").unwrap();
        std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b"lixo\n").unwrap();
        registrar(&p, t("2026-10-31T11:00:00Z"), "X1", None, "end").unwrap();
        let e = ler_eventos(&p);
        assert_eq!(e.len(), 2);
        assert_eq!(nomes(&e)["X1"], "Fazer X");
    }

    #[test]
    fn item_ativo_pula_humano_onhold_e_fechados() {
        let d = tempfile::tempdir().unwrap();
        let od = d.path().join(".schematize/overdev");
        std::fs::create_dir_all(&od).unwrap();
        std::fs::write(
            od.join("CHECKLIST.md"),
            "- [x] A1 feito\n- [H ] A2 humano\n- [~] A3 parado\n- [ ] **A8** — gates que mentem\n- [ ] A9 depois\n",
        )
        .unwrap();
        assert_eq!(item_ativo_overdev(d.path()).as_deref(), Some("overdev:A8"));
        assert_eq!(
            id_do_item(" Escrever o parser de custo (v2)").as_deref(),
            Some("overdev:escrever-o-parser-de-custo-v2")
        );
        let vazio = tempfile::tempdir().unwrap();
        assert!(item_ativo_overdev(vazio.path()).is_none());
    }
}
