//! O QUE: prova que o grafo que `scripts/build-index.py` gera e de fato LEGIVEL pelo parser
//! do proprio app (`panel::parse_graph`). O formato do indice (secao 39) e um CONTRATO com
//! esse parser, nao uma convencao de documentacao — se o gerador escrever de um jeito que o
//! parser nao le, o painel de grafos abre vazio e ninguem fica sabendo.
//!
//! POR QUE EXISTE: na v0.50.1 o parser aceitava prosa com seta como adjacencia e um projeto
//! real virou 127 nos, a maioria lixo. O risco espelhado — e o que este teste cobre — e o
//! GERADOR regredir: emitir linha que o parser nao reconhece, derrubando em silencio a
//! contagem de nos. A assercao e por CONTAGEM (secao 39: o grafo enumera, nao resume).
//!
//! DE ONDE VEM: `.schematize/grafos/` do workspace (um nivel acima do repo do CLI).
//! PRA ONDE VAI: so assercao — nao escreve nada.

use std::path::PathBuf;

/// Diretorio do grafo vivo do workspace. `None` quando o CLI e clonado fora do umbrella —
/// ai o teste se declara inaplicavel em vez de falhar por ambiente.
fn workspace_grafos() -> Option<PathBuf> {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent()?.join(".schematize/grafos");
    d.is_dir().then_some(d)
}

/// Soma os totais que os proprios grafos DECLARAM no cabecalho (`Total: **N unidades**`).
/// E o N do gate de completude — o alvo que o parser tem que conseguir recuperar.
fn total_declarado(dir: &std::path::Path) -> usize {
    let mut soma = 0;
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let txt = std::fs::read_to_string(e.path()).unwrap_or_default();
        for l in txt.lines() {
            if let Some(p) = l.find("Total: **") {
                let rest = &l[p + "Total: **".len()..];
                let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                soma += num.parse::<usize>().unwrap_or(0);
            }
        }
    }
    soma
}

#[test]
fn parser_recupera_o_grafo_que_o_gerador_escreveu() {
    let Some(dir) = workspace_grafos() else {
        eprintln!("sem .schematize/grafos/ no workspace — teste inaplicavel aqui");
        return;
    };
    let declarado = total_declarado(&dir);
    assert!(declarado > 0, "nenhum grafo declara total — cabecalho da secao 39 sumiu?");

    let (nodes, edges) = schematize::panel::parse_graph(&dir);

    // Conta so os nos COM localizacao. E de proposito: no sem `loc` o parser tambem cria a
    // partir da lista de adjacencia, entao contar `nodes.len()` mascararia a regressao (a
    // tabela podia parar de ser lida inteira que o total nao se mexia). `arquivo:linha` so
    // vem da TABELA — e o que prova que `parse_func_row` leu cada unidade enumerada.
    let com_loc = nodes.iter().filter(|n| n.loc.is_some()).count();
    assert!(
        com_loc >= declarado,
        "parser leu {com_loc} nos COM arquivo:linha, mas os grafos declaram {declarado} \
         unidades: o gerador emitiu linha de tabela que o `parse_func_row` nao reconhece"
    );
    assert!(!edges.is_empty(), "nenhuma aresta lida — adjacencia ASCII quebrada?");
}
