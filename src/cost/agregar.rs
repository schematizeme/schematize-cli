//! Agregações do ledger (dia, semana ISO, modelo, task, sessão, agente) e o `custos.json`.
//!
//! O JSON é CONTRATO com a GUI (§4): campos só se somam, nunca mudam de nome ou tipo.

use super::ledger::Linha;
use super::parser::Avisos;
use super::preco::Tabela;
use super::{Tokens, AVISO_ESTIMATIVA};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Acumulador de uma linha de tabela.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tot {
    pub inp: u64,
    pub out: u64,
    pub cache_w: u64,
    pub cache_r: u64,
    /// Soma só do que TEM preço.
    pub usd: f64,
    pub respostas: u64,
    pub sem_preco: u64,
}

impl Tot {
    pub fn total(&self) -> u64 {
        self.inp + self.out + self.cache_w + self.cache_r
    }

    /// `None` quando NADA tem preço (não finge zero); parcial soma o que dá.
    pub fn usd(&self) -> Option<f64> {
        (self.respostas > self.sem_preco).then_some(arredondar(self.usd))
    }

    fn somar(&mut self, l: &Linha, usd: Option<f64>) {
        self.inp += l.inp;
        self.out += l.out;
        self.cache_w += l.cache_w;
        self.cache_r += l.cache_r;
        self.respostas += 1;
        match usd {
            Some(u) => self.usd += u,
            None => self.sem_preco += 1,
        }
    }

    fn json(&self) -> Value {
        json!({
            "tokens": {"in": self.inp, "out": self.out, "cache_w": self.cache_w,
                       "cache_r": self.cache_r, "total": self.total()},
            "usd": self.usd(),
            "respostas": self.respostas,
            "sem_preco": self.sem_preco,
        })
    }
}

/// 6 casas: o suficiente pra não perder centavo de resposta pequena, sem ruído de float no git.
fn arredondar(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

/// Uma linha de tabela: `chave` é o dia/modelo/task/… conforme a lista.
#[derive(Debug, Clone)]
pub struct Item {
    pub chave: String,
    /// Nome legível (só `por_task`).
    pub nome: Option<String>,
    /// Último `ts` (só `por_sessao`).
    pub ultimo_ts: Option<String>,
    pub tot: Tot,
}

#[derive(Debug, Clone)]
pub struct Custos {
    pub gerado_em: String,
    pub projeto: String,
    pub pricing_versao: String,
    pub fuso: String,
    pub avisos: Vec<String>,
    pub total: Tot,
    pub por_modelo: Vec<Item>,
    pub por_dia: Vec<Item>,
    pub por_semana: Vec<Item>,
    pub por_task: Vec<Item>,
    pub por_sessao: Vec<Item>,
    pub por_agente: Vec<Item>,
}

/// Tokens da linha como o `Tokens` do preço (pra reprecificar quando o preço chega depois).
fn tokens_de(l: &Linha) -> Tokens {
    let w1h = l.cache_w1h.min(l.cache_w);
    Tokens { inp: l.inp, out: l.out, cache_w5: l.cache_w - w1h, cache_w1h: w1h, cache_r: l.cache_r }
}

/// US$ de uma linha: o que o ledger gravou; senão, tenta o preço de HOJE (modelo que ganhou
/// preço depois do sync); senão `None`.
fn usd_da_linha(l: &Linha, tab: &Tabela) -> Option<f64> {
    l.usd.or_else(|| tab.custo(&l.modelo, &tokens_de(l)))
}

fn agrupar<F>(linhas: &[(Linha, Option<f64>)], chave: F) -> BTreeMap<String, Item>
where
    F: Fn(&Linha) -> Option<String>,
{
    let mut m: BTreeMap<String, Item> = BTreeMap::new();
    for (l, usd) in linhas {
        let Some(k) = chave(l) else { continue };
        let it = m.entry(k.clone()).or_insert_with(|| Item {
            chave: k,
            nome: None,
            ultimo_ts: None,
            tot: Tot::default(),
        });
        it.tot.somar(l, *usd);
        if it.ultimo_ts.as_ref().is_none_or(|t| *t < l.ts) {
            it.ultimo_ts = Some(l.ts.clone());
        }
    }
    m
}

/// Ordem das listas "ranking": mais caro primeiro (sem preço por último), depois mais tokens.
fn por_custo(m: BTreeMap<String, Item>) -> Vec<Item> {
    let mut v: Vec<Item> = m.into_values().collect();
    v.sort_by(|a, b| {
        let (ua, ub) = (a.tot.usd(), b.tot.usd());
        ub.partial_cmp(&ua)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.tot.total().cmp(&a.tot.total()))
            .then(a.chave.cmp(&b.chave))
    });
    v
}

/// Tudo que `montar` precisa (agrupado: são oito insumos).
pub struct Entrada<'a> {
    pub linhas: &'a [Linha],
    pub tab: &'a Tabela,
    pub avisos_parse: &'a Avisos,
    pub avisos_extra: Vec<String>,
    pub nomes_task: &'a BTreeMap<String, String>,
    pub projeto: &'a str,
    pub fuso: &'a str,
    pub gerado_em: &'a str,
}

/// Monta o relatório a partir do ledger JÁ consolidado.
pub fn montar(e: Entrada) -> Custos {
    let Entrada { linhas, tab, avisos_parse, avisos_extra, nomes_task, projeto, fuso, gerado_em } =
        e;
    let com_usd: Vec<(Linha, Option<f64>)> =
        linhas.iter().map(|l| (l.clone(), usd_da_linha(l, tab))).collect();

    let mut total = Tot::default();
    for (l, u) in &com_usd {
        total.somar(l, *u);
    }

    let mut avisos = avisos_extra;
    avisos.extend(avisos_parse.frases());
    let modelos = agrupar(&com_usd, |l| Some(l.modelo.clone()));
    for (m, it) in &modelos {
        match tab.buscar(m) {
            None => avisos.push(format!(
                "modelo sem preço: {m} ({} resposta(s)) — custo em US$ fica null",
                it.tot.respostas
            )),
            Some((_, p, _)) if !p.confirmado => {
                avisos.push(format!("preço não confirmado na fonte oficial: {m}"))
            }
            Some(_) => {}
        }
    }

    let mut por_task = agrupar(&com_usd, |l| Some(l.task.clone()));
    for it in por_task.values_mut() {
        it.nome = nomes_task.get(&it.chave).cloned();
    }
    let cron = |m: BTreeMap<String, Item>| m.into_values().collect::<Vec<_>>();
    Custos {
        gerado_em: gerado_em.to_string(),
        projeto: projeto.to_string(),
        pricing_versao: tab.versao.clone(),
        fuso: fuso.to_string(),
        avisos,
        total,
        por_modelo: por_custo(modelos),
        por_dia: cron(agrupar(&com_usd, |l| Some(l.dia.clone()))),
        por_semana: cron(agrupar(&com_usd, |l| Some(l.semana_iso.clone()))),
        por_task: por_custo(por_task),
        por_sessao: por_custo(agrupar(&com_usd, |l| Some(l.sessao.clone()))),
        por_agente: por_custo(agrupar(&com_usd, |l| l.agente.clone())),
    }
}

impl Custos {
    /// O `custos.json` do contrato §4.
    pub fn json(&self) -> Value {
        let lista = |campo: &str, v: &[Item]| -> Value {
            Value::Array(
                v.iter()
                    .map(|it| {
                        let mut o = match it.tot.json() {
                            Value::Object(o) => o,
                            _ => Map::new(),
                        };
                        o.insert(campo.to_string(), json!(it.chave));
                        if campo == "task" {
                            o.insert("nome".into(), json!(it.nome));
                        }
                        if campo == "sessao" {
                            o.insert("ultimo_ts".into(), json!(it.ultimo_ts));
                        }
                        Value::Object(o)
                    })
                    .collect(),
            )
        };
        json!({
            "versao": 1,
            "projeto": self.projeto,
            "gerado_em": self.gerado_em,
            "pricing_versao": self.pricing_versao,
            "fuso": self.fuso,
            "aviso_estimativa": AVISO_ESTIMATIVA,
            "avisos": self.avisos,
            "total": self.total.json(),
            "por_modelo": lista("modelo", &self.por_modelo),
            "por_dia": lista("dia", &self.por_dia),
            "por_semana": lista("semana", &self.por_semana),
            "por_task": lista("task", &self.por_task),
            "por_sessao": lista("sessao", &self.por_sessao),
            "por_agente": lista("agente", &self.por_agente),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::ledger::testes::linha;

    fn tab() -> Tabela {
        Tabela::embutida().unwrap()
    }

    fn montar_simples(ls: &[Linha]) -> Custos {
        montar(Entrada {
            linhas: ls,
            tab: &tab(),
            avisos_parse: &Avisos::default(),
            avisos_extra: vec![],
            nomes_task: &BTreeMap::new(),
            projeto: "p",
            fuso: "-03:00",
            gerado_em: "2026-10-31T00:00:00Z",
        })
    }

    #[test]
    fn totais_e_agrupamentos_por_dia_semana_modelo() {
        let mut a = linha("a", 10);
        let mut b = linha("b", 20);
        b.dia = "2026-11-02".into();
        b.semana_iso = "2026-W45".into();
        b.modelo = "claude-sonnet-5-5".into();
        b.usd = Some(0.25);
        a.usd = Some(0.5);
        let c = montar_simples(&[a, b]);
        assert_eq!(c.total.respostas, 2);
        assert_eq!(c.total.usd(), Some(0.75));
        assert_eq!(c.total.total(), 2 + 30);
        assert_eq!(
            c.por_dia.iter().map(|i| i.chave.as_str()).collect::<Vec<_>>(),
            ["2026-10-31", "2026-11-02"]
        );
        assert_eq!(c.por_semana.len(), 2);
        // ranking: o mais caro primeiro
        assert_eq!(c.por_modelo[0].chave, "claude-opus-5-5");
    }

    #[test]
    fn modelo_sem_preco_nao_inventa_usd_e_avisa() {
        let mut l = linha("a", 10);
        l.modelo = "claude-misterio-9".into();
        l.usd = None;
        let c = montar_simples(&[l]);
        assert_eq!(c.total.usd(), None);
        assert!(c.avisos.iter().any(|a| a.contains("modelo sem preço: claude-misterio-9")));
        assert!(c.json()["total"]["usd"].is_null());
    }

    #[test]
    fn preco_que_chega_depois_reprecifica_so_o_null() {
        let mut l = linha("a", 1_000_000);
        l.usd = None;
        l.inp = 0;
        let c = montar_simples(&[l]); // opus-5-5 agora tem preço: out 20/Mtok
        assert_eq!(c.total.usd(), Some(20.0));
    }

    #[test]
    fn subagent_soma_na_sessao_do_pai_e_aparece_por_agente() {
        let pai = linha("a", 10);
        let mut sub = linha("b", 30);
        sub.agente = Some("ag1".into());
        let c = montar_simples(&[pai, sub]);
        assert_eq!(c.por_sessao.len(), 1);
        assert_eq!(c.por_sessao[0].tot.out, 40);
        assert_eq!(c.por_agente.len(), 1);
        assert_eq!((c.por_agente[0].chave.as_str(), c.por_agente[0].tot.out), ("ag1", 30));
    }

    #[test]
    fn json_tem_o_formato_do_contrato() {
        let mut l = linha("a", 10);
        l.task = "T1".into();
        let mut nomes = BTreeMap::new();
        nomes.insert("T1".to_string(), "Tarefa um".to_string());
        let c = montar(Entrada {
            linhas: &[l],
            tab: &tab(),
            avisos_parse: &Avisos::default(),
            avisos_extra: vec![],
            nomes_task: &nomes,
            projeto: "p",
            fuso: "-03:00",
            gerado_em: "g",
        });
        let j = c.json();
        for k in [
            "gerado_em",
            "pricing_versao",
            "avisos",
            "total",
            "por_modelo",
            "por_dia",
            "por_semana",
            "por_task",
            "por_sessao",
            "por_agente",
        ] {
            assert!(j.get(k).is_some(), "falta {k}");
        }
        assert_eq!(j["total"]["tokens"]["total"], 11);
        assert_eq!(j["por_task"][0]["task"], "T1");
        assert_eq!(j["por_task"][0]["nome"], "Tarefa um");
        assert_eq!(j["por_dia"][0]["dia"], "2026-10-31");
        assert_eq!(j["por_semana"][0]["semana"], "2026-W44");
        assert_eq!(j["aviso_estimativa"], AVISO_ESTIMATIVA);
    }
}
