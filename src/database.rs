//! Backend do "database builder" — modelo, introspecção, geração e grafo de schema.
//! O quê: um modelo relacional simples (Serialize/Deserialize — a GUI passa/salva como JSON),
//! introspecção de bancos existentes (SQLite via rusqlite; Postgres via `psql` no PATH),
//! geração de SQL (`CREATE TABLE`/`ALTER … FK`/`CREATE INDEX`) e de migration expand-contract,
//! e a conversão pro grafo força-dirigido (tabela = nó, FK = aresta) que a GUI já renderiza.
//! Onde: chamado por `main` (`schematize db …`) e pela GUI (database builder). v1 pragmático,
//! sem crate async: Postgres roda `psql` por Command e parseia a saída. MySQL fica pra v2 (TODO).

use crate::panel::{Edge, Node};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

// ---------------------------------------------------------------------------
// Modelo (serializável) — a GUI carrega/salva o schema inteiro como JSON.
// ---------------------------------------------------------------------------

/// Schema relacional: só o conjunto de tabelas (v1). Raiz do JSON da GUI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub tables: Vec<Table>,
}

/// Uma tabela: nome + colunas, chaves estrangeiras e índices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub fks: Vec<Fk>,
    pub indexes: Vec<Index>,
}

/// Uma coluna: nome, tipo (texto do SQL), e flags de nulabilidade/chave/unicidade.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: String,
    pub nullable: bool,
    pub pk: bool,
    pub unique: bool,
}

/// Chave estrangeira: coluna local → (tabela, coluna) referenciada.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fk {
    pub column: String,
    pub ref_table: String,
    pub ref_column: String,
}

/// Índice: nome, colunas (na ordem) e se é UNIQUE.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

// ---------------------------------------------------------------------------
// Introspecção SQLite (rusqlite — já é dep, feature bundled).
// ---------------------------------------------------------------------------

/// Aspa dupla de identificador (escapando `"` interno) pra montar PRAGMA/DDL com segurança.
fn q(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Introspecta um arquivo SQLite: lista as tabelas de usuário e, por tabela, colunas
/// (`PRAGMA table_info`), FKs (`PRAGMA foreign_key_list`) e índices
/// (`PRAGMA index_list`/`index_info`). Marca `unique` na coluna quando há um índice
/// UNIQUE de coluna única sobre ela. Erro claro se o arquivo não abrir.
pub fn introspect_sqlite(path: &Path) -> Result<Schema, String> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|e| format!("não consegui abrir o SQLite {}: {e}", path.display()))?;
    let mut schema = Schema::default();

    let mut names: Vec<String> = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        for r in rows {
            names.push(r.map_err(|e| e.to_string())?);
        }
    }

    for tname in names {
        let mut table = Table { name: tname.clone(), ..Default::default() };

        // Colunas: cid, name, type, notnull, dflt_value, pk.
        {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({})", q(&tname)))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Column {
                        name: r.get::<_, String>(1)?,
                        ty: r.get::<_, String>(2).unwrap_or_default(),
                        nullable: r.get::<_, i64>(3)? == 0,
                        pk: r.get::<_, i64>(5)? > 0,
                        unique: false,
                    })
                })
                .map_err(|e| e.to_string())?;
            for r in rows {
                table.columns.push(r.map_err(|e| e.to_string())?);
            }
        }

        // FKs: id, seq, table, from, to, on_update, on_delete, match.
        {
            let mut stmt = conn
                .prepare(&format!("PRAGMA foreign_key_list({})", q(&tname)))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Fk {
                        ref_table: r.get::<_, String>(2)?,
                        column: r.get::<_, String>(3)?,
                        ref_column: r.get::<_, String>(4).unwrap_or_default(),
                    })
                })
                .map_err(|e| e.to_string())?;
            for r in rows {
                table.fks.push(r.map_err(|e| e.to_string())?);
            }
        }

        // Índices: index_list (seq, name, unique, origin, partial) + index_info (cols).
        {
            let mut idxs: Vec<(String, bool)> = Vec::new();
            {
                let mut stmt = conn
                    .prepare(&format!("PRAGMA index_list({})", q(&tname)))
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(2)? != 0)))
                    .map_err(|e| e.to_string())?;
                for r in rows {
                    idxs.push(r.map_err(|e| e.to_string())?);
                }
            }
            for (iname, uniq) in idxs {
                let mut cols: Vec<String> = Vec::new();
                let mut stmt = conn
                    .prepare(&format!("PRAGMA index_info({})", q(&iname)))
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, Option<String>>(2))
                    .map_err(|e| e.to_string())?;
                for r in rows {
                    if let Some(c) = r.map_err(|e| e.to_string())? {
                        cols.push(c);
                    }
                }
                table.indexes.push(Index { name: iname, columns: cols, unique: uniq });
            }
        }

        // Índice UNIQUE de coluna única → marca a coluna como unique.
        let unique_cols: Vec<String> = table
            .indexes
            .iter()
            .filter(|i| i.unique && i.columns.len() == 1)
            .map(|i| i.columns[0].clone())
            .collect();
        for c in &mut table.columns {
            if unique_cols.contains(&c.name) {
                c.unique = true;
            }
        }

        schema.tables.push(table);
    }
    Ok(schema)
}

// ---------------------------------------------------------------------------
// Introspecção Postgres via `psql` (sem crate novo).
// ---------------------------------------------------------------------------

/// Separador de campo pedido ao `psql` (`-F`): o caractere de Unit Separator, que não
/// aparece em identificadores/tipos — evita ambiguidade com `|` do modo unaligned.
const PSQL_SEP: char = '\u{1f}';

/// Parseia UMA linha da saída do `psql -tA -F<sep>`: quebra pelos campos e apara. PURA.
pub fn parse_psql_row(line: &str) -> Vec<String> {
    line.split(PSQL_SEP).map(|c| c.trim().to_string()).collect()
}

/// Roda `psql "<conn>" -tA -F<sep> -c "<query>"` e devolve as linhas já quebradas em campos.
/// Erro claro se o `psql` não estiver no PATH ou se o comando falhar (stderr do psql).
fn run_psql(conn: &str, query: &str) -> Result<Vec<Vec<String>>, String> {
    let out = Command::new("psql")
        .arg(conn)
        .arg("-tA")
        .arg("-F")
        .arg(PSQL_SEP.to_string())
        .arg("-c")
        .arg(query)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "`psql` não está no PATH — instale o cliente do PostgreSQL (postgresql-client).".to_string()
            } else {
                format!("falha ao rodar psql: {e}")
            }
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("psql falhou: {}", err.trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().filter(|l| !l.trim().is_empty()).map(parse_psql_row).collect())
}

/// Introspecta um Postgres pelo `psql` (schema `public`): colunas+nulabilidade
/// (`information_schema.columns`), PK e FK (`table_constraints`/`key_column_usage`/
/// `constraint_column_usage`) e índices (`pg_indexes`). Sem crate async — só Command.
/// TODO(v2): MySQL/MariaDB (via `mysql`/`information_schema` equivalente).
pub fn introspect_postgres(conn: &str) -> Result<Schema, String> {
    // Tabelas + colunas na ordem ordinal.
    let cols = run_psql(
        conn,
        "SELECT table_name, column_name, data_type, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema='public' ORDER BY table_name, ordinal_position",
    )?;

    let mut tables: Vec<Table> = Vec::new();
    let mut idx_by_name: HashMap<String, usize> = HashMap::new();
    for row in &cols {
        if row.len() < 4 {
            continue;
        }
        let (tname, cname, ty, nullable) = (&row[0], &row[1], &row[2], row[3] == "YES");
        let ti = *idx_by_name.entry(tname.clone()).or_insert_with(|| {
            tables.push(Table { name: tname.clone(), ..Default::default() });
            tables.len() - 1
        });
        tables[ti].columns.push(Column {
            name: cname.clone(),
            ty: ty.clone(),
            nullable,
            pk: false,
            unique: false,
        });
    }

    // Chaves primárias.
    let pks = run_psql(
        conn,
        "SELECT tc.table_name, kcu.column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name=kcu.constraint_name AND tc.table_schema=kcu.table_schema \
         WHERE tc.constraint_type='PRIMARY KEY' AND tc.table_schema='public'",
    )?;
    for row in &pks {
        if row.len() < 2 {
            continue;
        }
        if let Some(&ti) = idx_by_name.get(&row[0]) {
            if let Some(c) = tables[ti].columns.iter_mut().find(|c| c.name == row[1]) {
                c.pk = true;
            }
        }
    }

    // Chaves estrangeiras.
    let fks = run_psql(
        conn,
        "SELECT tc.table_name, kcu.column_name, ccu.table_name, ccu.column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name=kcu.constraint_name AND tc.table_schema=kcu.table_schema \
         JOIN information_schema.constraint_column_usage ccu \
           ON ccu.constraint_name=tc.constraint_name AND ccu.table_schema=tc.table_schema \
         WHERE tc.constraint_type='FOREIGN KEY' AND tc.table_schema='public'",
    )?;
    for row in &fks {
        if row.len() < 4 {
            continue;
        }
        if let Some(&ti) = idx_by_name.get(&row[0]) {
            tables[ti].fks.push(Fk {
                column: row[1].clone(),
                ref_table: row[2].clone(),
                ref_column: row[3].clone(),
            });
        }
    }

    // Índices (parseia colunas + UNIQUE do indexdef).
    let idxs = run_psql(
        conn,
        "SELECT tablename, indexname, indexdef FROM pg_indexes WHERE schemaname='public'",
    )?;
    for row in &idxs {
        if row.len() < 3 {
            continue;
        }
        if let Some(&ti) = idx_by_name.get(&row[0]) {
            let unique = row[2].to_uppercase().contains("UNIQUE INDEX");
            let columns = parse_indexdef_columns(&row[2]);
            tables[ti].indexes.push(Index { name: row[1].clone(), columns, unique });
        }
    }

    // UNIQUE de coluna única → marca a coluna.
    for t in &mut tables {
        let unique_cols: Vec<String> = t
            .indexes
            .iter()
            .filter(|i| i.unique && i.columns.len() == 1)
            .map(|i| i.columns[0].clone())
            .collect();
        for c in &mut t.columns {
            if unique_cols.contains(&c.name) {
                c.unique = true;
            }
        }
    }

    Ok(Schema { tables })
}

/// Extrai a lista de colunas de um `indexdef` do Postgres (o miolo entre parênteses,
/// tirando `ASC`/`DESC` e aspas). Ex.: `… ON t USING btree (a, b DESC)` → `[a, b]`.
fn parse_indexdef_columns(def: &str) -> Vec<String> {
    let Some(op) = def.rfind('(') else { return Vec::new() };
    let Some(cl) = def.rfind(')') else { return Vec::new() };
    if cl <= op + 1 {
        return Vec::new();
    }
    def[op + 1..cl]
        .split(',')
        .map(|c| {
            c.split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches('"')
                .to_string()
        })
        .filter(|c| !c.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Geração de SQL + migration.
// ---------------------------------------------------------------------------

/// Gera o SQL do schema: um `CREATE TABLE` por tabela (colunas, tipos, NOT NULL, PK,
/// UNIQUE — PK composta vira constraint de tabela), depois `ALTER TABLE … ADD FOREIGN
/// KEY` de cada FK e `CREATE [UNIQUE] INDEX` de cada índice não-automático.
pub fn to_sql(schema: &Schema) -> String {
    let mut s = String::new();
    for t in &schema.tables {
        s.push_str(&create_table(t));
        s.push('\n');
    }
    for t in &schema.tables {
        for fk in &t.fks {
            s.push_str(&format!(
                "ALTER TABLE {} ADD FOREIGN KEY ({}) REFERENCES {} ({});\n",
                q(&t.name),
                q(&fk.column),
                q(&fk.ref_table),
                q(&fk.ref_column)
            ));
        }
    }
    for t in &schema.tables {
        for idx in &t.indexes {
            // Pula índices auto-gerados (PK/UNIQUE já saem inline no CREATE TABLE).
            if idx.name.contains("autoindex") || idx.columns.is_empty() {
                continue;
            }
            let uniq = if idx.unique { "UNIQUE " } else { "" };
            let cols = idx.columns.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
            s.push_str(&format!(
                "CREATE {uniq}INDEX {} ON {} ({cols});\n",
                q(&idx.name),
                q(&t.name)
            ));
        }
    }
    s
}

/// Monta o `CREATE TABLE` de uma tabela (colunas inline + PK simples inline / composta
/// como constraint de tabela).
fn create_table(t: &Table) -> String {
    let pk_cols: Vec<&str> = t.columns.iter().filter(|c| c.pk).map(|c| c.name.as_str()).collect();
    let mut lines: Vec<String> = Vec::new();
    for c in &t.columns {
        let ty = if c.ty.trim().is_empty() { "TEXT" } else { c.ty.trim() };
        let mut l = format!("  {} {ty}", q(&c.name));
        if !c.nullable {
            l.push_str(" NOT NULL");
        }
        if pk_cols.len() == 1 && c.pk {
            l.push_str(" PRIMARY KEY");
        }
        if c.unique && !c.pk {
            l.push_str(" UNIQUE");
        }
        lines.push(l);
    }
    if pk_cols.len() > 1 {
        let cols = pk_cols.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
        lines.push(format!("  PRIMARY KEY ({cols})"));
    }
    format!("CREATE TABLE {} (\n{}\n);\n", q(&t.name), lines.join(",\n"))
}

/// Gera a migration expand-contract: bloco UP (o SQL completo) + bloco DOWN, que dropa
/// as tabelas na ORDEM REVERSA (contrai o que o UP expandiu). Reversível por desenho.
pub fn to_migration(schema: &Schema) -> String {
    let mut s = String::new();
    s.push_str("-- ============================ UP (expand) ============================\n");
    s.push_str(&to_sql(schema));
    s.push_str("\n-- ========================== DOWN (contract) ==========================\n");
    for t in schema.tables.iter().rev() {
        s.push_str(&format!("DROP TABLE IF EXISTS {};\n", q(&t.name)));
    }
    s
}

// ---------------------------------------------------------------------------
// Grafo (reusa panel::Node/Edge) + descrições por tabela.
// ---------------------------------------------------------------------------

/// Converte o schema no grafo força-dirigido da GUI: cada tabela é um NÓ (id = nome),
/// cada FK é uma ARESTA `tabela -> tabela_referenciada` (rótulo = coluna). Arestas
/// duplicadas (mesma origem/destino/coluna) são deduplicadas.
pub fn to_graph(schema: &Schema) -> (Vec<Node>, Vec<Edge>) {
    let nodes: Vec<Node> = schema
        .tables
        .iter()
        .map(|t| Node { id: t.name.clone(), loc: None })
        .collect();
    let mut edges: Vec<Edge> = Vec::new();
    let mut seen: std::collections::BTreeSet<(String, String, String)> = std::collections::BTreeSet::new();
    for t in &schema.tables {
        for fk in &t.fks {
            let key = (t.name.clone(), fk.ref_table.clone(), fk.column.clone());
            if seen.insert(key) {
                edges.push(Edge {
                    from: t.name.clone(),
                    to: fk.ref_table.clone(),
                    label: Some(fk.column.clone()),
                });
            }
        }
    }
    (nodes, edges)
}

/// Descrição por NÓ (tabela) pro clique no grafo: um resumo das colunas com marcadores
/// (`PK`/`UK`/`NULL`). Ex.: `id:INTEGER PK, email:TEXT UK, nome:TEXT`. Trunca em 12 colunas.
pub fn table_descriptions(schema: &Schema) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for t in &schema.tables {
        let mut parts: Vec<String> = Vec::new();
        for c in t.columns.iter().take(12) {
            let mut p = format!("{}:{}", c.name, if c.ty.is_empty() { "TEXT" } else { &c.ty });
            if c.pk {
                p.push_str(" PK");
            }
            if c.unique && !c.pk {
                p.push_str(" UK");
            }
            if c.nullable {
                p.push_str(" NULL");
            }
            parts.push(p);
        }
        if t.columns.len() > 12 {
            parts.push(format!("… (+{})", t.columns.len() - 12));
        }
        out.insert(t.name.clone(), parts.join(", "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cria um SQLite temporário com 2 tabelas (FK author→user), introspecta e confere.
    #[test]
    fn introspecta_sqlite_com_fk() {
        let path = std::env::temp_dir().join(format!(
            "schematize-dbtest-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE, name TEXT);
                 CREATE TABLE posts (
                    id INTEGER PRIMARY KEY,
                    author_id INTEGER NOT NULL REFERENCES users(id),
                    title TEXT NOT NULL
                 );
                 CREATE INDEX idx_posts_title ON posts(title);",
            )
            .unwrap();
        }
        let schema = introspect_sqlite(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(schema.tables.len(), 2, "2 tabelas");
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        let posts = schema.tables.iter().find(|t| t.name == "posts").unwrap();

        // users: id PK, email NOT NULL + UNIQUE.
        let id = users.columns.iter().find(|c| c.name == "id").unwrap();
        assert!(id.pk, "id é PK");
        let email = users.columns.iter().find(|c| c.name == "email").unwrap();
        assert!(!email.nullable, "email NOT NULL");
        assert!(email.unique, "email UNIQUE via índice de coluna única");

        // posts: FK author_id -> users(id).
        assert_eq!(posts.fks.len(), 1, "1 FK em posts");
        let fk = &posts.fks[0];
        assert_eq!(fk.column, "author_id");
        assert_eq!(fk.ref_table, "users");
        assert_eq!(fk.ref_column, "id");

        // índice não-único idx_posts_title presente.
        assert!(posts.indexes.iter().any(|i| i.name == "idx_posts_title" && !i.unique));
    }

    fn schema_sintetico() -> Schema {
        Schema {
            tables: vec![
                Table {
                    name: "users".into(),
                    columns: vec![
                        Column { name: "id".into(), ty: "BIGINT".into(), nullable: false, pk: true, unique: false },
                        Column { name: "email".into(), ty: "TEXT".into(), nullable: false, pk: false, unique: true },
                    ],
                    fks: vec![],
                    indexes: vec![],
                },
                Table {
                    name: "posts".into(),
                    columns: vec![
                        Column { name: "id".into(), ty: "BIGINT".into(), nullable: false, pk: true, unique: false },
                        Column { name: "author_id".into(), ty: "BIGINT".into(), nullable: false, pk: false, unique: false },
                    ],
                    fks: vec![Fk { column: "author_id".into(), ref_table: "users".into(), ref_column: "id".into() }],
                    indexes: vec![Index { name: "idx_posts_author".into(), columns: vec!["author_id".into()], unique: false }],
                },
            ],
        }
    }

    #[test]
    fn to_sql_emite_create_alter_index() {
        let sql = to_sql(&schema_sintetico());
        assert!(sql.contains("CREATE TABLE \"users\""));
        assert!(sql.contains("\"id\" BIGINT NOT NULL PRIMARY KEY"));
        assert!(sql.contains("\"email\" TEXT NOT NULL UNIQUE"));
        assert!(sql.contains("ALTER TABLE \"posts\" ADD FOREIGN KEY (\"author_id\") REFERENCES \"users\" (\"id\");"));
        assert!(sql.contains("CREATE INDEX \"idx_posts_author\" ON \"posts\" (\"author_id\");"));
    }

    #[test]
    fn to_migration_tem_up_e_down_reverso() {
        let m = to_migration(&schema_sintetico());
        assert!(m.contains("UP (expand)"));
        assert!(m.contains("DOWN (contract)"));
        // DOWN dropa na ordem reversa: posts antes de users.
        let down = &m[m.find("DOWN").unwrap()..];
        let p = down.find("DROP TABLE IF EXISTS \"posts\"").unwrap();
        let u = down.find("DROP TABLE IF EXISTS \"users\"").unwrap();
        assert!(p < u, "posts é dropada antes de users (ordem reversa)");
    }

    #[test]
    fn to_graph_tabela_no_fk_aresta() {
        let (nodes, edges) = to_graph(&schema_sintetico());
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().any(|n| n.id == "users"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from, "posts");
        assert_eq!(edges[0].to, "users");
        assert_eq!(edges[0].label.as_deref(), Some("author_id"));
    }

    #[test]
    fn table_descriptions_resume_colunas() {
        let d = table_descriptions(&schema_sintetico());
        let users = d.get("users").unwrap();
        assert!(users.contains("id:BIGINT PK"));
        assert!(users.contains("email:TEXT UK"));
    }

    #[test]
    fn parse_psql_row_quebra_campos() {
        let line = format!("users{sep}email{sep}text{sep}NO", sep = PSQL_SEP);
        let cells = parse_psql_row(&line);
        assert_eq!(cells, vec!["users", "email", "text", "NO"]);
    }

    #[test]
    fn parse_indexdef_pega_colunas() {
        let def = "CREATE UNIQUE INDEX idx ON public.users USING btree (email)";
        assert_eq!(parse_indexdef_columns(def), vec!["email".to_string()]);
        let def2 = "CREATE INDEX idx ON public.posts USING btree (author_id, created_at DESC)";
        assert_eq!(parse_indexdef_columns(def2), vec!["author_id".to_string(), "created_at".to_string()]);
    }
}
