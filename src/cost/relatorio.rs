//! `custos.md`: o relatório humano, derivado do mesmo [`Custos`] que gera o JSON.

use super::agregar::{Custos, Item, Tot};
use super::AVISO_ESTIMATIVA;
use std::fmt::Write;

/// Milhar com ponto (pt-BR): `1234567` → `1.234.567`.
pub fn milhar(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// `US$ 12,34` (4 casas abaixo de 1 centavo); sem preço: `—`.
pub fn usd(v: Option<f64>) -> String {
    match v {
        None => "—".to_string(),
        Some(x) if x != 0.0 && x.abs() < 0.01 => format!("US$ {:.4}", x).replace('.', ","),
        Some(x) => format!("US$ {:.2}", x).replace('.', ","),
    }
}

fn linha(t: &Tot) -> String {
    format!(
        "{} | {} | {} | {} | {} | {}",
        usd(t.usd()),
        milhar(t.total()),
        milhar(t.inp),
        milhar(t.out),
        milhar(t.cache_w),
        milhar(t.cache_r)
    )
}

fn tabela(md: &mut String, titulo: &str, rotulo: &str, itens: &[Item], limite: Option<usize>) {
    let _ = writeln!(md, "\n## {titulo}\n");
    if itens.is_empty() {
        md.push_str("_sem dados_\n");
        return;
    }
    let _ = writeln!(md, "| {rotulo} | US$ | tokens | in | out | cache w | cache r |");
    md.push_str("|---|--:|--:|--:|--:|--:|--:|\n");
    for it in itens.iter().take(limite.unwrap_or(usize::MAX)) {
        let nome = match &it.nome {
            Some(n) => format!("{} ({n})", it.chave),
            None => it.chave.clone(),
        };
        let _ = writeln!(md, "| `{}` | {} |", nome.replace('|', "/"), linha(&it.tot));
    }
    if let Some(l) = limite.filter(|l| itens.len() > *l) {
        let _ = writeln!(md, "\n_top {l} de {}_", itens.len());
    }
}

pub fn markdown(c: &Custos) -> String {
    let mut md = String::new();
    let _ = writeln!(md, "# Custos — {}\n", c.projeto);
    let _ = writeln!(md, "> **Estimativa:** {AVISO_ESTIMATIVA}.");
    let _ = writeln!(
        md,
        "> Gerado em {} · preços `{}` · fuso {} · derivado de `ledger.jsonl` (regenerável: `schematize cost report`).\n",
        c.gerado_em, c.pricing_versao, c.fuso
    );
    let _ = writeln!(md, "## Total\n");
    let _ = writeln!(
        md,
        "- **{}** em {} tokens, {} respostas",
        usd(c.total.usd()),
        milhar(c.total.total()),
        milhar(c.total.respostas)
    );
    let _ = writeln!(
        md,
        "- in {} · out {} · cache escrita {} · cache leitura {}",
        milhar(c.total.inp),
        milhar(c.total.out),
        milhar(c.total.cache_w),
        milhar(c.total.cache_r)
    );
    tabela(&mut md, "Por modelo", "modelo", &c.por_modelo, None);
    tabela(&mut md, "Por dia (fuso local)", "dia", &c.por_dia, None);
    tabela(&mut md, "Por semana ISO", "semana", &c.por_semana, None);
    tabela(&mut md, "Por task", "task", &c.por_task, None);
    tabela(&mut md, "Top 10 sessões", "sessão", &c.por_sessao, Some(10));
    tabela(&mut md, "Top 10 agentes (subagents)", "agente", &c.por_agente, Some(10));
    if !c.avisos.is_empty() {
        md.push_str("\n## Avisos\n\n");
        for a in &c.avisos {
            let _ = writeln!(md, "- {a}");
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::agregar::{montar, Entrada};
    use crate::cost::ledger::testes::linha as l;
    use crate::cost::parser::Avisos;
    use crate::cost::preco::Tabela;
    use std::collections::BTreeMap;

    #[test]
    fn formatos() {
        assert_eq!(milhar(1_234_567), "1.234.567");
        assert_eq!(milhar(12), "12");
        assert_eq!(milhar(1000), "1.000");
        assert_eq!(usd(Some(12.345)), "US$ 12,35");
        assert_eq!(usd(Some(0.0042)), "US$ 0,0042");
        assert_eq!(usd(None), "—");
    }

    #[test]
    fn markdown_traz_ressalva_secoes_e_top10() {
        let ls: Vec<_> = (0..12)
            .map(|i| {
                let mut x = l(&format!("k{i}"), 10 + i);
                x.sessao = format!("s{i}");
                x
            })
            .collect();
        let c = montar(Entrada {
            linhas: &ls,
            tab: &Tabela::embutida().unwrap(),
            avisos_parse: &Avisos::default(),
            avisos_extra: vec![],
            nomes_task: &BTreeMap::new(),
            projeto: "proj",
            fuso: "-03:00",
            gerado_em: "2026-10-31T00:00:00Z",
        });
        let md = markdown(&c);
        assert!(md.contains("estimativa API-equivalente"));
        for s in [
            "## Total",
            "## Por modelo",
            "## Por dia",
            "## Por semana ISO",
            "## Por task",
            "## Top 10 sessões",
        ] {
            assert!(md.contains(s), "falta {s}");
        }
        assert!(md.contains("_top 10 de 12_"));
        assert!(md.contains("sem dados")); // agentes vazio
    }
}
