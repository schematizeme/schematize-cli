//! Histórico de commits e estado de push de um repositório git.
//! O quê: lista os últimos commits (marcando os que JÁ estão no upstream) e
//! resume o upstream do branch atual (ahead/behind). Provér-agnóstico de crate:
//! chama o `git` do sistema via `git -C <root> ...` e parseia a saída.
//! Onde: consumido pela GUI (painel de histórico) e pelo CLI `schematize git-log`.

use std::path::Path;
use std::process::Command;

/// Separador de campos usado no `--pretty` (0x1f, unit separator): não aparece
/// em mensagens de commit, então é seguro pra split.
const FS: char = '\u{1f}';

/// Um commit do log, com a marca de já-pushado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Hash completo (%H).
    pub hash: String,
    /// Hash curto (%h).
    pub short: String,
    /// Autor (%an).
    pub author: String,
    /// Data (%ad, formato --date=short → AAAA-MM-DD).
    pub date: String,
    /// Assunto/1ª linha da mensagem (%s).
    pub subject: String,
    /// `true` se o commit já está no upstream (foi pushado).
    pub pushed: bool,
}

/// Resumo do upstream do branch atual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    /// Branch local atual (ex.: `main`).
    pub branch: String,
    /// Ref de upstream (ex.: `origin/main`); None quando não há tracking.
    pub remote: Option<String>,
    /// Commits locais à frente do upstream (não pushados).
    pub ahead: usize,
    /// Commits do upstream ainda não integrados localmente.
    pub behind: usize,
}

/// Roda `git -C <root> <args>` e devolve o stdout (None se falhar/não for repo).
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
}

/// Parseia UMA linha do log no formato `%H\x1f%h\x1f%an\x1f%ad\x1f%s`.
/// PURA e testável. None se a linha não tem os 5 campos ou o hash é vazio.
pub fn parse_commit_line(line: &str) -> Option<Commit> {
    let mut it = line.split(FS);
    let hash = it.next()?.to_string();
    let short = it.next()?.to_string();
    let author = it.next()?.to_string();
    let date = it.next()?.to_string();
    let subject = it.next()?.to_string();
    if hash.is_empty() {
        return None;
    }
    Some(Commit { hash, short, author, date, subject, pushed: false })
}

/// Últimos `limit` commits de `root` (mais recentes primeiro). Vazio se `root`
/// não for um repositório git. Marca `pushed=true` nos que já estão no upstream.
pub fn commits(root: &Path, limit: usize) -> Vec<Commit> {
    let n = format!("-n{limit}");
    let raw = match git(root, &["log", &n, &format!("--pretty=format:%H{FS}%h{FS}%an{FS}%ad{FS}%s"), "--date=short"]) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut list: Vec<Commit> = raw.lines().filter_map(parse_commit_line).collect();

    // Só dá pra distinguir pushado se houver upstream. Com upstream, o conjunto
    // `@{u}..HEAD` são os NÃO-pushados; o resto está no upstream.
    if upstream(root).is_some() {
        if let Some(unpushed) = git(root, &["log", "@{u}..HEAD", "--pretty=%H"]) {
            let set: std::collections::HashSet<&str> =
                unpushed.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            for c in &mut list {
                c.pushed = !set.contains(c.hash.as_str());
            }
        } else {
            // Upstream existe mas a query falhou: assume tudo pushado (conservador
            // pra não sugerir push de coisa já enviada).
            for c in &mut list {
                c.pushed = true;
            }
        }
    }
    list
}

/// Estado do upstream do branch atual de `root`. None se não for repo git, se
/// não houver HEAD, ou se o branch não tiver upstream (tracking) configurado.
pub fn upstream(root: &Path) -> Option<Upstream> {
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
    // Sem tracking, `@{u}` falha → None (sem upstream).
    let remote = git(root, &["rev-parse", "--abbrev-ref", "@{u}"])?.trim().to_string();
    if remote.is_empty() {
        return None;
    }
    // `--left-right --count @{u}...HEAD` → "<behind>\t<ahead>" (esquerda=upstream).
    let (ahead, behind) = match git(root, &["rev-list", "--left-right", "--count", "@{u}...HEAD"]) {
        Some(s) => {
            let mut it = s.split_whitespace();
            let behind = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let ahead = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            (ahead, behind)
        }
        None => (0, 0),
    };
    Some(Upstream { branch, remote: Some(remote), ahead, behind })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn parseia_linha_de_log() {
        let line = "abc123def\x1fabc123d\x1fLucas\x1f2026-08-16\x1ffeat: faz X";
        let c = parse_commit_line(line).unwrap();
        assert_eq!(c.hash, "abc123def");
        assert_eq!(c.short, "abc123d");
        assert_eq!(c.author, "Lucas");
        assert_eq!(c.date, "2026-08-16");
        assert_eq!(c.subject, "feat: faz X");
        assert!(!c.pushed, "por padrão não-pushado até cruzar com o upstream");
    }

    #[test]
    fn subject_com_separador_extra_nao_quebra() {
        // Assunto normal (sem 0x1f). Campos a mais seriam ignorados pelo split→next.
        let line = "h\x1fs\x1fA\x1f2026-01-01\x1fassunto: com dois pontos";
        let c = parse_commit_line(line).unwrap();
        assert_eq!(c.subject, "assunto: com dois pontos");
    }

    #[test]
    fn linha_incompleta_ou_vazia_e_none() {
        assert!(parse_commit_line("").is_none(), "linha vazia (hash vazio)");
        assert!(parse_commit_line("so\x1fdois").is_none(), "faltam campos");
    }

    fn run(root: &Path, args: &[&str]) {
        let ok = Command::new("git").arg("-C").arg(root).args(args).status().unwrap().success();
        assert!(ok, "git {args:?} falhou");
    }

    #[test]
    fn repo_com_um_commit_sem_upstream() {
        let dir = std::env::temp_dir().join(format!("schematize-githist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        run(&dir, &["init", "-q"]);
        run(&dir, &["config", "user.email", "t@t.dev"]);
        run(&dir, &["config", "user.name", "T"]);
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        run(&dir, &["add", "a.txt"]);
        run(&dir, &["commit", "-q", "-m", "primeiro commit"]);

        let cs = commits(&dir, 20);
        assert_eq!(cs.len(), 1, "1 commit no repo");
        assert_eq!(cs[0].subject, "primeiro commit");
        assert!(!cs[0].pushed, "sem upstream → não pushado");

        assert!(upstream(&dir).is_none(), "sem tracking → upstream None");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_sem_git_devolve_vazio() {
        let dir = std::env::temp_dir().join(format!("schematize-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(commits(&dir, 5).is_empty(), "não é repo git → vazio");
        assert!(upstream(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
