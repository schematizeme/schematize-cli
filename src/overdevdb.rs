//! DB LOCAL do overdev (SQLite) — cópia versionada dos artefatos de `.overdev/`.
//! O quê: o agente (Claude) pode editar/apagar os arquivos de controle dentro do
//! projeto; este DB guarda snapshots versionados pra nada se perder. Grava uma
//! nova linha só quando o conteúdo muda (dedup por hash), permite listar o
//! histórico, ler um snapshot e restaurar o conteúdo no caminho original.
//! Onde: gancho best-effort no `overdev` (start/note/human_done) e CLI
//! `schematize overdev snapshot|history|restore`. DB em `~/.schematize/overdev.db`
//! (override por env `SCHEMATIZE_OVERDEV_DB`, usado nos testes).

use crate::util::home_app_dir;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Metadados de um snapshot (SEM o conteúdo) — pra listar o histórico.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMeta {
    /// Id (rowid) do snapshot — chave pra `get`/`restore`.
    pub id: i64,
    /// Caminho do arquivo RELATIVO à raiz do projeto (ex.: `.overdev/CHECKLIST.md`).
    pub file: String,
    /// Momento da gravação (epoch secs).
    pub ts: i64,
    /// Hash do conteúdo (dedup de versões idênticas).
    pub hash: String,
    /// Tamanho do conteúdo em bytes.
    pub size: i64,
}

/// Caminho do DB. Respeita o override `SCHEMATIZE_OVERDEV_DB` (testes); senão
/// `~/.schematize/overdev.db` (mesma convenção de HOME do `config`/`util`).
fn db_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SCHEMATIZE_OVERDEV_DB") {
        return PathBuf::from(p);
    }
    home_app_dir().join("overdev.db")
}

/// Segundos desde a época (sem depender de crate de data).
fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Hash barato e determinístico do conteúdo (dedup de versões). Não é
/// criptográfico — só distingue conteúdos diferentes pra não duplicar linha.
fn hash_content(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Abre a conexão (cria o diretório e a tabela se preciso).
fn open() -> Result<Connection, String> {
    let path = db_path();
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project TEXT,
            file TEXT,
            ts INTEGER,
            hash TEXT,
            size INTEGER,
            content TEXT
        )",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

/// Caminhos RELATIVOS (à raiz) que o snapshot cobre e que existem agora:
/// os artefatos de `.overdev/` (CHECKLIST/PLAN/DECISOES/OBJETIVO/DESIGN-*/notes/*
/// e state.json) mais o `PERGUNTAS-OVERDEV.txt` da raiz.
fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut rels: Vec<PathBuf> = Vec::new();
    // Dir de overdev resolvido (`.schematize/overdev` novo ou `.overdev` legado); o RELATIVO
    // à raiz vira a chave armazenada no DB, então snapshots de projeto novo/legado ficam distintos.
    let od = crate::paths::overdev_dir_at(root);
    let od_rel = od.strip_prefix(root).unwrap_or(Path::new(".schematize/overdev")).to_path_buf();
    let od_rel = od_rel.as_path();

    // Arquivos fixos de controle.
    for name in ["CHECKLIST.md", "PLAN.md", "DECISOES.md", "OBJETIVO.md", "NOTAS.md", "state.json"]
    {
        let rel = od_rel.join(name);
        if root.join(&rel).is_file() {
            rels.push(rel);
        }
    }
    // DESIGN-*.md soltos no .overdev.
    if let Ok(rd) = fs::read_dir(&od) {
        let mut designs: Vec<PathBuf> = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name();
                let n = name.to_string_lossy();
                if n.starts_with("DESIGN-") && n.ends_with(".md") && e.path().is_file() {
                    Some(od_rel.join(name))
                } else {
                    None
                }
            })
            .collect();
        designs.sort();
        rels.extend(designs);
    }
    // notes/* (arquivos diretos).
    if let Ok(rd) = fs::read_dir(od.join("notes")) {
        let mut notes: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| od_rel.join("notes").join(e.file_name()))
            .collect();
        notes.sort();
        rels.extend(notes);
    }
    // PERGUNTAS-OVERDEV.txt na raiz.
    let perg = PathBuf::from("PERGUNTAS-OVERDEV.txt");
    if root.join(&perg).is_file() {
        rels.push(perg);
    }
    rels
}

/// Varre os artefatos de `<root>/.overdev/` (+ `PERGUNTAS-OVERDEV.txt`) e grava
/// um snapshot novo pra cada arquivo cujo hash mudou desde o último daquele
/// `(project, file)`. Retorna quantas linhas novas gravou.
pub fn snapshot(project_root: &Path) -> Result<usize, String> {
    let conn = open()?;
    let project = project_root.to_string_lossy().to_string();
    let ts = now_secs();
    let mut gravados = 0usize;
    for rel in collect_files(project_root) {
        let content = match fs::read_to_string(project_root.join(&rel)) {
            Ok(c) => c,
            Err(_) => continue, // ilegível/binário: ignora, best-effort
        };
        let file = rel.to_string_lossy().to_string();
        let hash = hash_content(&content);
        // Último hash gravado daquele (project, file).
        let last: Option<String> = conn
            .query_row(
                "SELECT hash FROM snapshots WHERE project = ?1 AND file = ?2 ORDER BY id DESC LIMIT 1",
                params![project, file],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if last.as_deref() == Some(hash.as_str()) {
            continue; // sem mudança: não duplica
        }
        conn.execute(
            "INSERT INTO snapshots (project, file, ts, hash, size, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![project, file, ts, hash, content.len() as i64, content],
        )
        .map_err(|e| e.to_string())?;
        gravados += 1;
    }
    Ok(gravados)
}

/// Histórico do projeto (metadados, mais recentes primeiro), até `limit`.
pub fn history(project_root: &Path, limit: usize) -> Result<Vec<SnapshotMeta>, String> {
    let conn = open()?;
    let project = project_root.to_string_lossy().to_string();
    let mut stmt = conn
        .prepare("SELECT id, file, ts, hash, size FROM snapshots WHERE project = ?1 ORDER BY id DESC LIMIT ?2")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![project, limit as i64], |r| {
            Ok(SnapshotMeta {
                id: r.get(0)?,
                file: r.get(1)?,
                ts: r.get(2)?,
                hash: r.get(3)?,
                size: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Conteúdo de um snapshot pelo id.
pub fn get(id: i64) -> Result<String, String> {
    let conn = open()?;
    conn.query_row("SELECT content FROM snapshots WHERE id = ?1", params![id], |r| r.get(0))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => format!("snapshot {id} não encontrado"),
            other => other.to_string(),
        })
}

/// Restaura o conteúdo do snapshot `id` no caminho original (`<root>/<file>`),
/// criando os diretórios necessários. Retorna o caminho escrito.
pub fn restore(id: i64, project_root: &Path) -> Result<PathBuf, String> {
    let conn = open()?;
    let (file, content): (String, String) = conn
        .query_row("SELECT file, content FROM snapshots WHERE id = ?1", params![id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => format!("snapshot {id} não encontrado"),
            other => other.to_string(),
        })?;
    let dest = project_root.join(&file);
    if let Some(d) = dest.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    fs::write(&dest, content).map_err(|e| e.to_string())?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    // Serializa os testes que tocam o DB: eles compartilham a env var global
    // SCHEMATIZE_OVERDEV_DB, então não podem rodar em paralelo.
    static DB_LOCK: Mutex<()> = Mutex::new(());
    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// Aponta o DB pra um arquivo temporário único e devolve um dir de projeto novo.
    fn fresh() -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let base =
            std::env::temp_dir().join(format!("schematize-odb-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(".overdev")).unwrap();
        std::env::set_var("SCHEMATIZE_OVERDEV_DB", base.join("overdev.db"));
        base
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn snapshot_grava_e_nao_duplica_sem_mudanca() {
        let _g = DB_LOCK.lock().unwrap();
        let root = fresh();
        write(&root, ".overdev/CHECKLIST.md", "- [ ] a\n");
        write(&root, ".overdev/state.json", "{}");
        write(&root, "PERGUNTAS-OVERDEV.txt", "pergunta 1\n");

        let n = snapshot(&root).unwrap();
        assert_eq!(n, 3, "3 arquivos novos no 1º snapshot");

        // Sem mudança: não grava nada.
        let n2 = snapshot(&root).unwrap();
        assert_eq!(n2, 0, "conteúdo idêntico não duplica");

        let h = history(&root, 50).unwrap();
        assert_eq!(h.len(), 3, "histórico tem só as 3 linhas");
    }

    #[test]
    fn mudar_arquivo_gera_nova_versao() {
        let _g = DB_LOCK.lock().unwrap();
        let root = fresh();
        write(&root, ".overdev/CHECKLIST.md", "- [ ] a\n");
        assert_eq!(snapshot(&root).unwrap(), 1);

        write(&root, ".overdev/CHECKLIST.md", "- [x] a\n- [ ] b\n");
        assert_eq!(snapshot(&root).unwrap(), 1, "hash mudou → nova versão");

        let h = history(&root, 50).unwrap();
        assert_eq!(h.len(), 2, "2 versões do mesmo arquivo");
        // Mais recente primeiro.
        assert!(h[0].id > h[1].id);
        assert_eq!(h[0].file, ".overdev/CHECKLIST.md");
        let novo = get(h[0].id).unwrap();
        assert!(novo.contains("- [ ] b"));
        let velho = get(h[1].id).unwrap();
        assert_eq!(velho, "- [ ] a\n");
    }

    #[test]
    fn restore_reescreve_o_caminho_original() {
        let _g = DB_LOCK.lock().unwrap();
        let root = fresh();
        write(&root, ".overdev/CHECKLIST.md", "versao 1\n");
        assert_eq!(snapshot(&root).unwrap(), 1);
        let id_v1 = history(&root, 10).unwrap()[0].id;

        // O agente "estraga" o arquivo.
        write(&root, ".overdev/CHECKLIST.md", "LIXO\n");
        let dest = restore(id_v1, &root).unwrap();
        assert_eq!(dest, root.join(".overdev/CHECKLIST.md"));
        assert_eq!(fs::read_to_string(&dest).unwrap(), "versao 1\n", "restaurou o conteúdo");
    }

    #[test]
    fn restore_cria_dirs_se_apagados() {
        let _g = DB_LOCK.lock().unwrap();
        let root = fresh();
        write(&root, ".overdev/notes/n1.md", "nota\n");
        assert_eq!(snapshot(&root).unwrap(), 1);
        let id = history(&root, 10).unwrap()[0].id;

        // Apaga o dir inteiro do .overdev.
        fs::remove_dir_all(root.join(".overdev")).unwrap();
        let dest = restore(id, &root).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "nota\n");
    }

    #[test]
    fn get_de_id_inexistente_erra() {
        let _g = DB_LOCK.lock().unwrap();
        let _root = fresh();
        assert!(get(999_999).is_err());
    }
}
