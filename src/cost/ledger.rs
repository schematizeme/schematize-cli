//! Persistência em `<archive>/custos/`: o ledger append-only, o cursor por arquivo, a escrita
//! atômica dos derivados e a trava que impede dois `sync` simultâneos de duplicar linhas.

use super::parser::Avisos;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Uma resposta deduplicada. A ordem dos campos é a do contrato §4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Linha {
    pub ts: String,
    pub dia: String,
    pub semana_iso: String,
    pub sessao: String,
    pub agente: Option<String>,
    pub modelo: String,
    pub task: String,
    #[serde(rename = "in")]
    pub inp: u64,
    pub out: u64,
    pub cache_w: u64,
    pub cache_r: u64,
    pub usd: Option<f64>,
    /// `message.id` — a idempotência do arquivo.
    pub chave: String,
    /// Parte de `cache_w` que é escrita de 1h (o resto é 5m).
    #[serde(default)]
    pub cache_w1h: u64,
}

/// Caminhos de `custos/`.
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn ledger(&self) -> PathBuf {
        self.0.join("ledger.jsonl")
    }
    pub fn cursor(&self) -> PathBuf {
        self.0.join(".cursor.json")
    }
    pub fn tasks(&self) -> PathBuf {
        self.0.join("tasks.jsonl")
    }
    pub fn json(&self) -> PathBuf {
        self.0.join("custos.json")
    }
    pub fn md(&self) -> PathBuf {
        self.0.join("custos.md")
    }
    pub fn pricing(&self) -> PathBuf {
        self.0.join("pricing.toml")
    }
    pub fn trava(&self) -> PathBuf {
        self.0.join(".lock")
    }
}

/// Lê o ledger tolerando linha corrompida (conta e segue). Ausente = vazio.
pub fn ler(path: &Path) -> Result<(Vec<Linha>, u64), String> {
    let txt = match std::fs::read(path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], 0)),
        Err(e) => return Err(format!("ler {}: {e}", path.display())),
    };
    let mut ruins = 0;
    let mut v = Vec::new();
    for l in txt.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Linha>(l) {
            Ok(x) => v.push(x),
            Err(_) => ruins += 1,
        }
    }
    Ok((v, ruins))
}

/// A leitura "lógica": por `chave` vale a ÚLTIMA linha (correção de streaming), na ordem da
/// primeira aparição.
pub fn consolidar(linhas: Vec<Linha>) -> Vec<Linha> {
    let mut pos: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Linha> = Vec::new();
    for l in linhas {
        match pos.get(&l.chave) {
            Some(&i) => out[i] = l,
            None => {
                pos.insert(l.chave.clone(), out.len());
                out.push(l);
            }
        }
    }
    out
}

/// Anexa linhas ao fim do ledger (cria o arquivo). Se o arquivo terminou sem `\n` (escrita
/// interrompida), fecha a linha quebrada antes — ela vira "inválida" e não gruda na nova.
pub fn anexar(path: &Path, novas: &[Linha]) -> Result<(), String> {
    if novas.is_empty() {
        return Ok(());
    }
    let mut buf = String::new();
    if let Ok(b) = std::fs::read(path) {
        if b.last().is_some_and(|&c| c != b'\n') {
            buf.push('\n');
        }
    }
    for l in novas {
        buf.push_str(&serde_json::to_string(l).map_err(|e| format!("serializar linha: {e}"))?);
        buf.push('\n');
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("abrir {}: {e}", path.display()))?;
    f.write_all(buf.as_bytes()).map_err(|e| format!("gravar {}: {e}", path.display()))?;
    f.sync_all().map_err(|e| format!("sync {}: {e}", path.display()))
}

/// Cursor: até que byte de cada transcript já foi lido, + os avisos acumulados.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cursor {
    #[serde(default)]
    pub versao: u32,
    /// Caminho relativo a `~/.claude/projects` → offset em bytes.
    #[serde(default)]
    pub arquivos: BTreeMap<String, u64>,
    #[serde(default)]
    pub avisos: Avisos,
}

impl Cursor {
    /// Cursor ilegível = recomeça do zero (o dedupe do ledger torna isso seguro, só mais lento).
    pub fn carregar(path: &Path) -> Cursor {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn salvar(&mut self, path: &Path) -> Result<(), String> {
        self.versao = 1;
        let j = serde_json::to_vec_pretty(self).map_err(|e| format!("cursor: {e}"))?;
        escrever_atomico(path, &j)
    }
}

/// Escrita atômica: temporário no MESMO diretório + rename (um leitor nunca vê meio arquivo).
pub fn escrever_atomico(path: &Path, dados: &[u8]) -> Result<(), String> {
    let nome = path.file_name().and_then(|n| n.to_str()).unwrap_or("arquivo");
    let tmp = path.with_file_name(format!(".{nome}.tmp-{}", std::process::id()));
    let r = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(dados)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("gravar {}: {e}", path.display()));
    }
    Ok(())
}

/// Trava exclusiva por arquivo (`create_new`). Solta no `Drop`.
pub struct Trava(PathBuf);

impl Trava {
    /// Espera até ~5 s; trava com mais de 60 s é de processo morto e é tomada.
    pub fn pegar(path: &Path) -> Result<Trava, String> {
        for _ in 0..50 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(mut f) => {
                    let _ = writeln!(f, "{}", std::process::id());
                    return Ok(Trava(path.to_path_buf()));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let velha = std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|d| d > Duration::from_secs(60));
                    if velha {
                        let _ = std::fs::remove_file(path);
                    } else {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                Err(e) => return Err(format!("trava {}: {e}", path.display())),
            }
        }
        Err(format!("outro `cost sync` está rodando ({})", path.display()))
    }
}

impl Drop for Trava {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;

    pub fn linha(chave: &str, out: u64) -> Linha {
        Linha {
            ts: "2026-10-31T10:00:00.000Z".into(),
            dia: "2026-10-31".into(),
            semana_iso: "2026-W44".into(),
            sessao: "s1".into(),
            agente: None,
            modelo: "claude-opus-5-5".into(),
            task: "sem-task".into(),
            inp: 1,
            out,
            cache_w: 0,
            cache_r: 0,
            usd: Some(0.5),
            chave: chave.into(),
            cache_w1h: 0,
        }
    }

    #[test]
    fn append_le_de_volta_e_corrompida_e_contada() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ledger.jsonl");
        anexar(&p, &[linha("a", 1), linha("b", 2)]).unwrap();
        // simula escrita interrompida: meia linha sem \n
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b"{\"ts\":\"x")
            .unwrap();
        anexar(&p, &[linha("c", 3)]).unwrap();
        let (v, ruins) = ler(&p).unwrap();
        assert_eq!(v.iter().map(|l| l.chave.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(ruins, 1);
    }

    #[test]
    fn consolidar_vale_a_ultima_linha_da_chave() {
        let v = consolidar(vec![linha("a", 1), linha("b", 2), linha("a", 9)]);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].chave.as_str(), v[0].out), ("a", 9));
    }

    #[test]
    fn ledger_ausente_e_vazio() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(ler(&d.path().join("nada")).unwrap(), (vec![], 0));
    }

    #[test]
    fn escrita_atomica_nao_deixa_temporario() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.json");
        escrever_atomico(&p, b"1").unwrap();
        escrever_atomico(&p, b"2").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"2");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn trava_e_exclusiva_e_solta_no_drop() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".lock");
        let t = Trava::pegar(&p).unwrap();
        assert!(p.exists());
        drop(t);
        assert!(!p.exists());
        assert!(Trava::pegar(&p).is_ok());
    }

    #[test]
    fn cursor_ilegivel_recomeca_do_zero() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".cursor.json");
        std::fs::write(&p, "lixo").unwrap();
        assert!(Cursor::carregar(&p).arquivos.is_empty());
        let mut c = Cursor::default();
        c.arquivos.insert("a".into(), 7);
        c.salvar(&p).unwrap();
        assert_eq!(Cursor::carregar(&p).arquivos["a"], 7);
    }
}
