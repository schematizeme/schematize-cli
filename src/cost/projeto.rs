//! Resolução do projeto e do archive, e localização dos transcripts dele.
//!
//! Projeto = a raiz que tem um `<projeto>_archive/` (sobe diretórios a partir do cwd). Sem
//! archive o projeto é ignorado — a menos que o usuário o passe EXPLÍCITO em `--projeto`.

use super::ledger::Dir;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Projeto {
    pub root: PathBuf,
    pub archive: PathBuf,
}

impl Projeto {
    pub fn nome(&self) -> String {
        self.root.file_name().and_then(|n| n.to_str()).unwrap_or("projeto").to_string()
    }

    /// `<archive>/custos/`.
    pub fn custos(&self) -> Dir {
        Dir(self.archive.join("custos"))
    }
}

/// O archive de `dir`: `<nome>_archive`, ou o ÚNICO filho `*_archive` (o nome do archive é o
/// prefixo comum dos microserviços, nem sempre o basename da pasta).
pub fn achar_archive(dir: &Path) -> Option<PathBuf> {
    let nome = dir.file_name()?.to_str()?;
    let direto = dir.join(format!("{nome}_archive"));
    if direto.is_dir() {
        return Some(direto);
    }
    let mut cands: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("_archive"))
        })
        .collect();
    cands.sort();
    (cands.len() == 1).then(|| cands.remove(0))
}

/// Sobe de `inicio` até achar uma raiz com archive.
pub fn do_diretorio(inicio: &Path) -> Option<Projeto> {
    let mut cur = Some(inicio);
    while let Some(d) = cur {
        if let Some(archive) = achar_archive(d) {
            return Some(Projeto { root: d.to_path_buf(), archive });
        }
        cur = d.parent();
    }
    None
}

/// Resolve `--projeto` (caminho, ou nome entre `catalogo` = projetos conhecidos do app) ou, sem
/// ele, o cwd. Explícito sem archive: cria `<nome>_archive/` (o usuário pediu esse projeto).
pub fn resolver(
    arg: Option<&str>,
    cwd: &Path,
    catalogo: &[(String, PathBuf)],
) -> Result<Projeto, String> {
    let Some(arg) = arg else {
        return do_diretorio(cwd).ok_or_else(|| {
            format!(
                "nenhum projeto com `<projeto>_archive/` a partir de {} — use --projeto <caminho>",
                cwd.display()
            )
        });
    };
    let p = Path::new(arg);
    let alvo = if p.is_dir() {
        std::fs::canonicalize(p).map_err(|e| format!("{arg}: {e}"))?
    } else if let Some((_, c)) = catalogo.iter().find(|(n, _)| n == arg) {
        c.clone()
    } else {
        return Err(format!(
            "projeto `{arg}` não encontrado (nem como pasta, nem entre os projetos do app)"
        ));
    };
    if let Some(pr) = do_diretorio(&alvo) {
        return Ok(pr);
    }
    let nome = alvo.file_name().and_then(|n| n.to_str()).ok_or("projeto sem nome")?;
    let archive = alvo.join(format!("{nome}_archive"));
    std::fs::create_dir_all(&archive).map_err(|e| format!("criar {}: {e}", archive.display()))?;
    Ok(Projeto { root: alvo, archive })
}

/// Como o Claude Code nomeia a pasta de transcripts de um cwd: todo caractere fora de
/// `[A-Za-z0-9-]` vira `-` (`/home/a_b.c` → `-home-a-b-c`).
pub fn slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

/// Um transcript a ler.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub abs: PathBuf,
    /// Relativo a `~/.claude/projects` — a chave do cursor.
    pub rel: String,
    pub sessao: String,
    pub agente: Option<String>,
}

/// Todos os `*.jsonl` das pastas de transcript do projeto: a do slug exato e as de subpastas
/// dele (`<slug>-…`), incluindo `<sessao>/subagents/agent-<id>.jsonl`. Ordem determinística.
///
/// O casamento por prefixo pode pegar um projeto irmão (`proj` vs `proj2`); o filtro por `cwd`
/// de cada resposta (no `sync`) fecha essa porta.
pub fn transcripts(claude_projects: &Path, root: &Path) -> Vec<Transcript> {
    let s = slug(root);
    let prefixo = format!("{s}-");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(claude_projects)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n == s || n.starts_with(&prefixo))
        })
        .collect();
    dirs.sort();
    let mut out = Vec::new();
    for d in dirs {
        coletar(&d, claude_projects, &mut out, 0);
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn coletar(dir: &Path, base: &Path, out: &mut Vec<Transcript>, prof: usize) {
    if prof > 4 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            coletar(&p, base, out, prof + 1);
        } else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            let Ok(rel) = p.strip_prefix(base) else { continue };
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let sub = p.parent().and_then(|d| d.file_name()).and_then(|n| n.to_str())
                == Some("subagents");
            let (sessao, agente) = if sub {
                let sess = p
                    .parent()
                    .and_then(Path::parent)
                    .and_then(|d| d.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                (sess, Some(stem.strip_prefix("agent-").unwrap_or(&stem).to_string()))
            } else {
                (stem, None)
            };
            out.push(Transcript {
                abs: p.clone(),
                rel: rel.to_string_lossy().into_owned(),
                sessao,
                agente,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_como_o_claude_code() {
        assert_eq!(
            slug(Path::new("/home/tom/indev/schematize_skills")),
            "-home-tom-indev-schematize-skills"
        );
        assert_eq!(slug(Path::new("/a/b.c")), "-a-b-c");
    }

    #[test]
    fn sobe_ate_a_raiz_com_archive_e_ignora_sem() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("meuprojeto");
        std::fs::create_dir_all(proj.join("meuprojeto_archive")).unwrap();
        std::fs::create_dir_all(proj.join("src/fundo")).unwrap();
        let p = do_diretorio(&proj.join("src/fundo")).unwrap();
        assert_eq!(p.root, proj);
        assert!(do_diretorio(&d.path().join("outro")).is_none());
        // sem --projeto e sem archive: erro
        assert!(resolver(None, d.path(), &[]).is_err());
    }

    #[test]
    fn archive_com_nome_diferente_do_basename_se_for_unico() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("guarda-chuva");
        std::fs::create_dir_all(proj.join("svc_archive")).unwrap();
        assert_eq!(achar_archive(&proj).unwrap(), proj.join("svc_archive"));
        std::fs::create_dir_all(proj.join("outro_archive")).unwrap();
        assert!(achar_archive(&proj).is_none());
    }

    #[test]
    fn explicito_sem_archive_cria() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("novo");
        std::fs::create_dir_all(&proj).unwrap();
        let p = resolver(proj.to_str(), d.path(), &[]).unwrap();
        assert!(p.archive.ends_with("novo_archive") && p.archive.is_dir());
    }

    #[test]
    fn acha_transcripts_da_sessao_e_dos_subagents() {
        let d = tempfile::tempdir().unwrap();
        let root = Path::new("/work/proj");
        let pasta = d.path().join("-work-proj");
        std::fs::create_dir_all(pasta.join("sess1/subagents")).unwrap();
        std::fs::create_dir_all(d.path().join("-work-proj-sub")).unwrap();
        std::fs::create_dir_all(d.path().join("-work-proj2")).unwrap();
        std::fs::write(pasta.join("sess1.jsonl"), "").unwrap();
        std::fs::write(pasta.join("sess1/subagents/agent-abc.jsonl"), "").unwrap();
        std::fs::write(pasta.join("sess1/subagents/agent-abc.meta.json"), "{}").unwrap();
        std::fs::write(d.path().join("-work-proj-sub/s2.jsonl"), "").unwrap();
        std::fs::write(d.path().join("-work-proj2/s3.jsonl"), "").unwrap();
        let t = transcripts(d.path(), root);
        let v: Vec<_> = t.iter().map(|x| (x.sessao.as_str(), x.agente.as_deref())).collect();
        assert_eq!(v, [("s2", None), ("sess1", None), ("sess1", Some("abc"))]);
    }
}
