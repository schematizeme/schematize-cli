//! Parser TOLERANTE dos transcripts do Claude Code (`*.jsonl`).
//!
//! Linha inválida ou sem campo essencial é pulada e contada em [`Avisos`]; nunca derruba a
//! leitura nem perde o resto do arquivo. Só `type = "assistant"` com `message.usage` conta.

use super::tempo::parse_ts;
use super::Tokens;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Contadores do que foi pulado. Acumulam entre `sync`s (ficam no cursor).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Avisos {
    /// Linhas que não são JSON.
    pub linhas_invalidas: u64,
    /// Respostas de assistant sem `usage`.
    pub sem_usage: u64,
    /// Respostas sem `id`, `model` ou `timestamp` legível.
    pub sem_campo: u64,
}

impl Avisos {
    pub fn somar(&mut self, o: &Avisos) {
        self.linhas_invalidas += o.linhas_invalidas;
        self.sem_usage += o.sem_usage;
        self.sem_campo += o.sem_campo;
    }

    /// Frases para o `avisos[]` do relatório (vazio se nada foi pulado).
    pub fn frases(&self) -> Vec<String> {
        let mut v = Vec::new();
        let mut add = |n: u64, txt: &str| {
            if n > 0 {
                v.push(format!("{n} {txt}"));
            }
        };
        add(self.linhas_invalidas, "linha(s) de transcript inválida(s) ignorada(s)");
        add(self.sem_usage, "resposta(s) sem usage ignorada(s)");
        add(self.sem_campo, "resposta(s) sem id/model/timestamp ignorada(s)");
        v
    }
}

/// De onde o arquivo vem — supre o que a linha não traz.
#[derive(Debug, Clone, Default)]
pub struct CtxArquivo {
    /// Sessão pelo caminho (`<sessao>.jsonl` ou `<sessao>/subagents/…`).
    pub sessao: String,
    /// `agent-<id>.jsonl` → `<id>`.
    pub agente: Option<String>,
}

/// Uma resposta já extraída (ainda sem dedupe).
#[derive(Debug, Clone, PartialEq)]
pub struct Resposta {
    /// `message.id` — a chave de dedupe.
    pub chave: String,
    pub ts: DateTime<Utc>,
    pub sessao: String,
    pub agente: Option<String>,
    pub modelo: String,
    pub tokens: Tokens,
    pub cwd: Option<String>,
}

fn n(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn texto<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Separa a escrita de cache em 5m/1h. Sem o detalhe `cache_creation`, tudo é 5m (o padrão
/// antigo da API); se o detalhe não fecha com o total, a diferença vai pra 5m.
fn cache_escrita(u: &Value) -> (u64, u64) {
    let det = u.get("cache_creation");
    let w1h_det = det.map(|d| n(d, "ephemeral_1h_input_tokens")).unwrap_or(0);
    let w5_det = det.map(|d| n(d, "ephemeral_5m_input_tokens")).unwrap_or(0);
    let total = n(u, "cache_creation_input_tokens").max(w1h_det + w5_det);
    let w1h = w1h_det.min(total);
    (total - w1h, w1h)
}

/// Lê UMA linha. `None` = nada a contar (linha de outro tipo, ou pulada — e então `av` sobe).
pub fn ler_linha(linha: &str, ctx: &CtxArquivo, av: &mut Avisos) -> Option<Resposta> {
    if linha.trim().is_empty() {
        return None;
    }
    let Ok(v) = serde_json::from_str::<Value>(linha) else {
        av.linhas_invalidas += 1;
        return None;
    };
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let msg = v.get("message")?;
    // Resposta sintética do próprio Claude Code (erro local, interrupção): não passou pela API.
    if texto(msg, "model") == Some("<synthetic>") {
        return None;
    }
    let Some(usage) = msg.get("usage").filter(|u| u.is_object()) else {
        av.sem_usage += 1;
        return None;
    };
    let (Some(id), Some(modelo), Some(ts)) =
        (texto(msg, "id"), texto(msg, "model"), texto(&v, "timestamp").and_then(parse_ts))
    else {
        av.sem_campo += 1;
        return None;
    };
    let (cache_w5, cache_w1h) = cache_escrita(usage);
    let sessao = texto(&v, "sessionId")
        .or_else(|| texto(&v, "session_id"))
        .map(str::to_string)
        .unwrap_or_else(|| ctx.sessao.clone());
    let agente = texto(&v, "agentId").map(str::to_string).or_else(|| ctx.agente.clone());
    Some(Resposta {
        chave: id.to_string(),
        ts,
        sessao,
        agente,
        modelo: modelo.to_string(),
        tokens: Tokens {
            inp: n(usage, "input_tokens"),
            out: n(usage, "output_tokens"),
            cache_w5,
            cache_w1h,
            cache_r: n(usage, "cache_read_input_tokens"),
        },
        cwd: texto(&v, "cwd").map(str::to_string),
    })
}

/// Dedupe de streaming: uma resposta por `chave`, a de MAIOR `output_tokens` (a última linha do
/// streaming é a completa). Mantém a ordem da primeira aparição.
pub fn dedupe(rs: Vec<Resposta>) -> Vec<Resposta> {
    let mut pos: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Resposta> = Vec::new();
    for r in rs {
        match pos.get(&r.chave) {
            Some(&i) => {
                if r.tokens.out > out[i].tokens.out {
                    out[i] = r;
                }
            }
            None => {
                pos.insert(r.chave.clone(), out.len());
                out.push(r);
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Geradores de linhas SINTÉTICAS no formato real do transcript — nenhum texto de conversa.

    pub fn assistant(
        id: &str,
        model: &str,
        ts: &str,
        inp: u64,
        out: u64,
        cw: u64,
        cr: u64,
    ) -> String {
        format!(
            r#"{{"type":"assistant","isSidechain":false,"sessionId":"sess-1","requestId":"req_{id}","timestamp":"{ts}","cwd":"/work/proj","message":{{"id":"{id}","model":"{model}","role":"assistant","usage":{{"input_tokens":{inp},"output_tokens":{out},"cache_creation_input_tokens":{cw},"cache_read_input_tokens":{cr},"cache_creation":{{"ephemeral_5m_input_tokens":{cw},"ephemeral_1h_input_tokens":0}}}}}}}}"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::assistant;
    use super::*;

    fn ctx() -> CtxArquivo {
        CtxArquivo { sessao: "sess-arq".into(), agente: None }
    }

    #[test]
    fn extrai_as_cinco_rubricas() {
        let l = r#"{"type":"assistant","sessionId":"s","timestamp":"2026-10-31T10:00:00.000Z","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":3,"output_tokens":40,"cache_creation_input_tokens":100,"cache_read_input_tokens":500,"cache_creation":{"ephemeral_5m_input_tokens":30,"ephemeral_1h_input_tokens":70}}}}"#;
        let mut av = Avisos::default();
        let r = ler_linha(l, &ctx(), &mut av).unwrap();
        assert_eq!(r.tokens, Tokens { inp: 3, out: 40, cache_w5: 30, cache_w1h: 70, cache_r: 500 });
        assert_eq!(av, Avisos::default());
    }

    #[test]
    fn sem_detalhe_de_cache_tudo_e_5m() {
        let l = r#"{"type":"assistant","timestamp":"2026-10-31T10:00:00Z","message":{"id":"m1","model":"x","usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":90}}}"#;
        let r = ler_linha(l, &ctx(), &mut Avisos::default()).unwrap();
        assert_eq!((r.tokens.cache_w5, r.tokens.cache_w1h), (90, 0));
        assert_eq!(r.sessao, "sess-arq");
    }

    #[test]
    fn dedupe_de_streaming_fica_com_o_maior_output() {
        let rs: Vec<_> = [10, 80, 40]
            .iter()
            .map(|o| {
                ler_linha(
                    &assistant("m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 2, *o, 5, 5),
                    &ctx(),
                    &mut Avisos::default(),
                )
                .unwrap()
            })
            .chain(std::iter::once(
                ler_linha(
                    &assistant("m2", "claude-opus-5-5", "2026-10-31T10:01:00Z", 2, 7, 0, 0),
                    &ctx(),
                    &mut Avisos::default(),
                )
                .unwrap(),
            ))
            .collect();
        let d = dedupe(rs);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].chave.as_str(), d[0].tokens.out), ("m1", 80));
        assert_eq!(d[1].chave, "m2");
    }

    #[test]
    fn linhas_ruins_sao_contadas_e_o_resto_segue() {
        let ok = assistant("m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 1, 1, 0, 0);
        let sem_usage = r#"{"type":"assistant","timestamp":"2026-10-31T10:00:00Z","message":{"id":"m2","model":"x"}}"#;
        let sem_id = r#"{"type":"assistant","timestamp":"2026-10-31T10:00:00Z","message":{"model":"x","usage":{"input_tokens":1}}}"#;
        let ts_ruim = r#"{"type":"assistant","timestamp":"ontem","message":{"id":"m3","model":"x","usage":{}}}"#;
        let user = r#"{"type":"user","message":{"content":"oi"}}"#;
        let sintetica = r#"{"type":"assistant","timestamp":"2026-10-31T10:00:00Z","message":{"id":"m4","model":"<synthetic>","usage":{"input_tokens":0}}}"#;
        let mut av = Avisos::default();
        let mut got = Vec::new();
        for l in ["{quebrada", "", &ok, sem_usage, sem_id, ts_ruim, user, sintetica, "[1,2"] {
            got.extend(ler_linha(l, &ctx(), &mut av));
        }
        assert_eq!(got.len(), 1);
        assert_eq!(av, Avisos { linhas_invalidas: 2, sem_usage: 1, sem_campo: 2 });
        assert_eq!(av.frases().len(), 3);
    }

    #[test]
    fn agente_vem_da_linha_ou_do_arquivo() {
        let l = assistant("m1", "x", "2026-10-31T10:00:00Z", 1, 1, 0, 0);
        let c = CtxArquivo { sessao: "s".into(), agente: Some("ab12".into()) };
        assert_eq!(
            ler_linha(&l, &c, &mut Avisos::default()).unwrap().agente.as_deref(),
            Some("ab12")
        );
        let com = l.replacen(r#""type":"assistant""#, r#""type":"assistant","agentId":"zz9""#, 1);
        assert_eq!(
            ler_linha(&com, &c, &mut Avisos::default()).unwrap().agente.as_deref(),
            Some("zz9")
        );
    }
}
