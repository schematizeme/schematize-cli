//! Subcomandos do DATABASE builder: origem do schema, resumo e o dispatch.

use crate::cli::args::*;
use schematize::database;

/// Resolve a fonte do schema pro `db sql|graph`: --from <json> | --sqlite | --postgres.
pub(crate) fn db_source(
    from: Option<String>,
    sqlite: Option<String>,
    postgres: Option<String>,
) -> Result<database::Schema, String> {
    if let Some(f) = from {
        let s = std::fs::read_to_string(&f).map_err(|e| format!("ler {f}: {e}"))?;
        return serde_json::from_str(&s).map_err(|e| format!("schema JSON inválido em {f}: {e}"));
    }
    if let Some(p) = sqlite {
        return database::introspect_sqlite(std::path::Path::new(&p));
    }
    if let Some(c) = postgres {
        return database::introspect_postgres(&c);
    }
    Err("informe a fonte do schema: --from <schema.json> | --sqlite <arquivo> | --postgres <conn>"
        .into())
}

/// Imprime o resumo humano de um schema (tabelas, nº de colunas/FKs/índices + totais).
pub(crate) fn db_print_summary(schema: &database::Schema) {
    let mut cols = 0usize;
    let mut fks = 0usize;
    for t in &schema.tables {
        cols += t.columns.len();
        fks += t.fks.len();
        let pk: Vec<&str> = t.columns.iter().filter(|c| c.pk).map(|c| c.name.as_str()).collect();
        println!(
            "  {} — {} coluna(s), {} FK(s), {} índice(s){}",
            t.name,
            t.columns.len(),
            t.fks.len(),
            t.indexes.len(),
            if pk.is_empty() { String::new() } else { format!("; PK: {}", pk.join(", ")) }
        );
    }
    println!("total: {} tabela(s), {cols} coluna(s), {fks} FK(s).", schema.tables.len());
}

/// `schematize db <sub>` — backend do database builder (introspect | sql | graph).
pub(crate) fn db_cmd(sub: DbCmd) -> Result<(), String> {
    match sub {
        DbCmd::Introspect { sqlite, postgres, json } => {
            let schema = db_source(None, sqlite, postgres)?;
            println!("Schema ({} tabela(s)):", schema.tables.len());
            db_print_summary(&schema);
            if json {
                let js = serde_json::to_string_pretty(&schema).map_err(|e| e.to_string())?;
                println!("\n--- schema.json ---\n{js}");
            }
            Ok(())
        }
        DbCmd::Sql { from, sqlite, postgres, migration } => {
            let schema = db_source(from, sqlite, postgres)?;
            if migration {
                print!("{}", database::to_migration(&schema));
            } else {
                print!("{}", database::to_sql(&schema));
            }
            Ok(())
        }
        DbCmd::Graph { from, sqlite, postgres } => {
            let schema = db_source(from, sqlite, postgres)?;
            let (nodes, edges) = database::to_graph(&schema);
            println!("nós ({}):", nodes.len());
            for n in &nodes {
                println!("  {}", n.id);
            }
            println!("arestas ({}):", edges.len());
            for e in &edges {
                match &e.label {
                    Some(l) => println!("  {} -> {} ({l})", e.from, e.to),
                    None => println!("  {} -> {}", e.from, e.to),
                }
            }
            Ok(())
        }
    }
}
