//! Tokens + modelo de um run, extraídos do TRANSCRIPT do Claude Code.
//! O quê: o Claude grava um `.jsonl` por sessão em `~/.claude/projects/<path-escapado>/`,
//! onde `<path-escapado>` é o caminho ABSOLUTO do projeto com cada caractere não
//! alfanumérico trocado por `-` (ex.: `/home/tom/proj/Adventury` -> `-home-tom-proj-Adventury`).
//! Cada linha é um JSON; mensagens trazem `message.usage` (input/output/cache) e `message.model`.
//! Onde: consumido pelo `schematize overdev status` (linha de tokens) e pela GUI.

use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Uso agregado de um projeto: tokens somados + modelos por nº de mensagens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// `input_tokens` somados.
    pub input: u64,
    /// `output_tokens` somados.
    pub output: u64,
    /// `cache_read_input_tokens` somados.
    pub cache_read: u64,
    /// `cache_creation_input_tokens` somados.
    pub cache_write: u64,
    /// `input + output` (tokens "de conteúdo", sem cache).
    pub total: u64,
    /// (modelo, nº de mensagens) — ordem DESC por nº de mensagens.
    pub models: Vec<(String, u64)>,
    /// Nº de mensagens com bloco `usage` contabilizadas.
    pub messages: u64,
}

impl Usage {
    /// Modelo principal (mais mensagens); `None` se não houve nenhuma mensagem.
    pub fn main_model(&self) -> Option<&str> {
        self.models.first().map(|(m, _)| m.as_str())
    }
}

/// Escapa o caminho absoluto pro nome do dir de projeto do Claude: cada caractere
/// que NÃO é `[A-Za-z0-9]` vira `-` (bate com `/`, `.`, `_`, etc.). PURO/testável.
fn escape_project_path(project: &Path) -> String {
    project
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Diretório de transcripts (`~/.claude/projects/<escapado>`) de um projeto.
/// Torna o `project` absoluto (junta ao cwd se vier relativo) antes de escapar.
fn project_transcript_dir(project: &Path) -> PathBuf {
    let abs = if project.is_absolute() {
        project.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(project)).unwrap_or_else(|_| project.to_path_buf())
    };
    util::claude_dir().join("projects").join(escape_project_path(&abs))
}

/// Uso agregado do projeto. Best-effort: dir inexistente/ilegível → `Usage` zerado.
/// PERF: pula (sem full-parse) qualquer linha que não contenha a substring `"usage"`
/// — os `.jsonl` passam de 100MB. Leitura linha-a-linha com BufReader.
pub fn agent_usage(project: &Path) -> Usage {
    usage_from_dir(&project_transcript_dir(project))
}

/// Núcleo injetável: varre os `*.jsonl` de `dir` e agrega o uso. Testado direto
/// com um tempdir (sem depender de `$HOME`/`~/.claude`).
fn usage_from_dir(dir: &Path) -> Usage {
    let mut u = Usage::default();
    let mut models: HashMap<String, u64> = HashMap::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return u; // dir inexistente → zerado
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(file) = File::open(&path) else { continue };
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            // PERF: só faz parse de linha que sinaliza um bloco de usage.
            if !line.contains("\"usage\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            let Some(msg) = v.get("message") else { continue };
            let Some(usage) = msg.get("usage") else { continue };
            let g = |k: &str| usage.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
            u.input += g("input_tokens");
            u.output += g("output_tokens");
            u.cache_read += g("cache_read_input_tokens");
            u.cache_write += g("cache_creation_input_tokens");
            u.messages += 1;
            if let Some(m) = msg.get("model").and_then(serde_json::Value::as_str) {
                if !m.is_empty() {
                    *models.entry(m.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    u.total = u.input + u.output;
    let mut mv: Vec<(String, u64)> = models.into_iter().collect();
    // DESC por nº de mensagens; desempate estável pelo nome do modelo.
    mv.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    u.models = mv;
    u
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn fresh_dir() -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("schematize-usage-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn escape_troca_nao_alfanumerico_por_traco() {
        assert_eq!(
            escape_project_path(Path::new("/home/tom/proj/Adventury")),
            "-home-tom-proj-Adventury"
        );
        // `.` e `_` também viram `-`.
        assert_eq!(escape_project_path(Path::new("/a/.b_c")), "-a--b-c");
    }

    #[test]
    fn soma_usage_conta_modelos_e_pula_linha_sem_usage() {
        let d = fresh_dir();
        let jsonl = concat!(
            // 1) opus com usage
            r#"{"message":{"model":"claude-opus-5","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":7}}}"#,
            "\n",
            // 2) linha SEM usage (deve ser pulada, mesmo tendo model)
            r#"{"message":{"model":"claude-opus-5"},"type":"whatever"}"#,
            "\n",
            // 3) sonnet com usage
            r#"{"message":{"model":"claude-sonnet-5","usage":{"input_tokens":20,"output_tokens":3,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            "\n",
            // 4) opus de novo (pra opus ficar com 2 msgs e ser o principal)
            r#"{"message":{"model":"claude-opus-5","usage":{"input_tokens":1,"output_tokens":1}}}"#,
            "\n",
            // linha lixo que nem é JSON mas não tem "usage" → pulada sem erro
            "isto nao e json\n",
        );
        std::fs::write(d.join("session.jsonl"), jsonl).unwrap();
        // arquivo não-jsonl é ignorado
        std::fs::write(d.join("outro.txt"), r#"{"message":{"usage":{"input_tokens":999}}}"#)
            .unwrap();

        let u = usage_from_dir(&d);
        assert_eq!(u.input, 31, "10+20+1");
        assert_eq!(u.output, 9, "5+3+1");
        assert_eq!(u.cache_read, 100);
        assert_eq!(u.cache_write, 7);
        assert_eq!(u.total, 40, "input+output");
        assert_eq!(u.messages, 3, "3 mensagens com usage (linha 2 pulada)");
        // opus tem 2 msgs, sonnet 1 → opus primeiro.
        assert_eq!(u.models.first().map(|(m, _)| m.as_str()), Some("claude-opus-5"));
        assert_eq!(u.models.iter().find(|(m, _)| m == "claude-opus-5").map(|(_, n)| *n), Some(2));
        assert_eq!(u.models.iter().find(|(m, _)| m == "claude-sonnet-5").map(|(_, n)| *n), Some(1));
        assert_eq!(u.main_model(), Some("claude-opus-5"));
    }

    #[test]
    fn dir_inexistente_da_usage_zerado() {
        let u = usage_from_dir(Path::new("/nao/existe/mesmo/xyz"));
        assert_eq!(u, Usage::default());
        assert_eq!(u.total, 0);
        assert!(u.models.is_empty());
    }
}
