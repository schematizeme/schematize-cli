//! REPOSITÓRIOS do serviço e o estado LOCAL de cada projeto.
//!
//! O quê: (a) lista os repositórios de uma conta no serviço, via `gh`; (b) resume o
//! estado local de cada projeto — qual conta está em uso, quantos commits faltam
//! enviar, se está sujo. Onde: `schematize git repos|status` e a tela Git da GUI.
//!
//! Sobre o (b), que é o que mais importa no dia a dia: git não guarda "histórico de
//! pushes" — não existe esse log. O que existe, e é a informação útil, é o que ainda
//! NÃO foi enviado: `@{u}..HEAD`. Um commit de identidade que só existe nesta máquina
//! some com a máquina, e é isso que este resumo torna visível de uma olhada.

use super::contas::Conta;
use crate::{githist, util};
use std::path::{Path, PathBuf};

/// Um repositório no serviço.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remoto {
    pub caminho: String,
    pub privado: bool,
    pub descricao: String,
    pub atualizado: String,
}

/// Lista os repositórios da conta via `gh`. Erro claro se o `gh` não estiver logado —
/// não inventamos lista vazia, que pareceria "você não tem repositório".
pub fn listar(c: &Conta, limite: usize) -> Result<Vec<Remoto>, String> {
    if util::run("gh", &["--version"]).is_err() {
        return Err(
            "o `gh` (GitHub CLI) não está instalado — necessário pra listar repositórios.".into()
        );
    }
    let n = limite.to_string();
    let saida = util::run(
        "gh",
        &["repo", "list", &c.usuario, "--limit", &n,
          "--json", "nameWithOwner,isPrivate,description,updatedAt",
          "--template", "{{range .}}{{.nameWithOwner}}\t{{.isPrivate}}\t{{.updatedAt}}\t{{.description}}\n{{end}}"],
    )
    .map_err(|e| format!("`gh repo list` falhou ({e}). Logado? `gh auth status`"))?;
    Ok(saida.lines().filter_map(parse_linha).collect())
}

/// Parseia UMA linha do template do `gh`. PURA — é onde mora o formato.
pub fn parse_linha(l: &str) -> Option<Remoto> {
    let mut c = l.split('\t');
    let caminho = c.next()?.trim().to_string();
    if caminho.is_empty() {
        return None;
    }
    let privado = c.next().unwrap_or("false").trim() == "true";
    let atualizado = c.next().unwrap_or("").trim().chars().take(10).collect();
    let descricao = c.next().unwrap_or("").trim().to_string();
    Some(Remoto { caminho, privado, descricao, atualizado })
}

/// Estado local de um projeto — o resumo que responde "o que ainda não saiu daqui".
#[derive(Debug, Clone)]
pub struct EstadoLocal {
    pub nome: String,
    pub raiz: PathBuf,
    /// Conta cadastrada em uso (None = nenhuma das cadastradas).
    pub conta: Option<String>,
    /// E-mail que o git usaria pra commitar aqui.
    pub email: String,
    /// Remoto de origem, se houver.
    pub remoto: Option<String>,
    /// Commits à frente do upstream — o que existe SÓ nesta máquina.
    pub nao_enviados: usize,
    /// Há alteração não commitada?
    pub sujo: bool,
}

/// Profundidade da busca por repositórios. Cobre `dev/umbrella/sub-repo/`.
const PROFUNDIDADE: usize = 5;

/// Todo diretório com `.git` sob os dev_dirs — inclusive SUB-REPOS.
///
/// Não dá pra usar o `projects::scan` aqui: ele para de descer no primeiro marcador,
/// então um guarda-chuva que é ele mesmo um projeto esconde os repositórios de dentro.
/// E é exatamente lá que mora o caso que esta tela existe pra pegar — o sub-repo com
/// commit que nunca saiu da máquina. Aqui a regra é outra: achou `.git`, é um repo;
/// registra e NÃO desce dentro (submódulo é assunto do repo pai).
pub fn repositorios(dev_dirs: &[String]) -> Vec<PathBuf> {
    fn desce(dir: &Path, nivel: usize, out: &mut Vec<PathBuf>) {
        if nivel > PROFUNDIDADE {
            return;
        }
        if dir.join(".git").exists() {
            out.push(dir.to_path_buf());
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            let nome = e.file_name();
            let nome = nome.to_string_lossy();
            // Ruído pesado: nunca vale descer, e um `node_modules` sozinho tem mais
            // diretórios que o resto da máquina inteira.
            if nome.starts_with('.')
                || matches!(nome.as_ref(), "node_modules" | "target" | "vendor" | "dist")
            {
                continue;
            }
            desce(&e.path(), nivel + 1, out);
        }
    }
    let mut v = Vec::new();
    for d in dev_dirs {
        desce(Path::new(d), 0, &mut v);
    }
    v.sort();
    v.dedup();
    v
}

/// Resume todos os repositórios dos diretórios de dev.
pub fn estado_dos_projetos(dev_dirs: &[String]) -> Vec<EstadoLocal> {
    let mut v: Vec<EstadoLocal> = repositorios(dev_dirs)
        .into_iter()
        .map(|p| {
            let nome = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            estado_de(&p, &nome)
        })
        .filter(|e| e.remoto.is_some() || e.nao_enviados > 0 || e.sujo)
        .collect();
    // O que está mais atrás de sair da máquina aparece primeiro.
    v.sort_by(|a, b| b.nao_enviados.cmp(&a.nao_enviados).then(b.sujo.cmp(&a.sujo)));
    v
}

/// Estado de UM projeto.
pub fn estado_de(raiz: &Path, nome: &str) -> EstadoLocal {
    let g = |args: &[&str]| -> Option<String> {
        let mut v = vec!["-C", raiz.to_str().unwrap_or(".")];
        v.extend_from_slice(args);
        util::run("git", &v).ok().map(|s| s.trim().to_string())
    };
    let up = githist::upstream(raiz);
    EstadoLocal {
        nome: nome.to_string(),
        raiz: raiz.to_path_buf(),
        conta: super::aplicar::conta_em_uso(raiz).map(|c| c.rotulo),
        email: g(&["config", "user.email"]).unwrap_or_default(),
        remoto: g(&["remote", "get-url", "origin"]),
        // Sem upstream, TODO commit local é "não enviado" — é o pior caso e o que
        // mais importa avisar (repo local que nunca foi pra lugar nenhum).
        nao_enviados: match &up {
            Some(u) => u.ahead,
            None => g(&["rev-list", "--count", "HEAD"]).and_then(|s| s.parse().ok()).unwrap_or(0),
        },
        sujo: g(&["status", "--porcelain"]).map(|s| !s.is_empty()).unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_linha_do_gh() {
        let r = parse_linha("org/repo\ttrue\t2026-08-19T10:00:00Z\tum projeto").unwrap();
        assert_eq!(r.caminho, "org/repo");
        assert!(r.privado);
        assert_eq!(r.atualizado, "2026-08-19", "só a data, sem a hora");
        assert_eq!(r.descricao, "um projeto");
    }

    /// Repositório sem descrição é comum — não pode virar `None`.
    #[test]
    fn sem_descricao_ainda_e_repo() {
        let r = parse_linha("org/repo\tfalse\t2026-01-01T00:00:00Z\t").unwrap();
        assert!(!r.privado);
        assert!(r.descricao.is_empty());
    }

    #[test]
    fn linha_vazia_nao_vira_repo() {
        assert!(parse_linha("").is_none());
        assert!(parse_linha("\ttrue\t\t").is_none());
    }

    /// Enxerga SUB-REPO dentro de um guarda-chuva que também é repo — o caso que o
    /// `projects::scan` esconde (ele para de descer no primeiro marcador) e que é
    /// justamente onde mora o commit que nunca saiu da máquina.
    #[test]
    fn enxerga_sub_repo_dentro_de_guarda_chuva() {
        let base = std::env::temp_dir().join(format!("gitwalk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        // guarda-chuva SEM .git, com dois repos dentro e ruído pesado no meio
        std::fs::create_dir_all(base.join("umbrella/repo_a/.git")).unwrap();
        std::fs::create_dir_all(base.join("umbrella/repo_b/.git")).unwrap();
        std::fs::create_dir_all(base.join("umbrella/repo_a/node_modules/x/.git")).unwrap();

        let achados = repositorios(&[base.display().to_string()]);
        assert_eq!(achados.len(), 2, "os dois sub-repos: {achados:?}");
        assert!(achados.iter().any(|p| p.ends_with("repo_a")));
        assert!(achados.iter().any(|p| p.ends_with("repo_b")));
        assert!(
            !achados.iter().any(|p| p.to_string_lossy().contains("node_modules")),
            "nunca desce em node_modules"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Achou `.git`, não desce dentro: submódulo é assunto do repo pai, e sem isso
    /// um repo com muitos submódulos vira dezenas de linhas repetidas.
    #[test]
    fn nao_desce_dentro_de_repo() {
        let base = std::env::temp_dir().join(format!("gitwalk2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("pai/.git")).unwrap();
        std::fs::create_dir_all(base.join("pai/sub/.git")).unwrap();
        let achados = repositorios(&[base.display().to_string()]);
        assert_eq!(achados.len(), 1);
        assert!(achados[0].ends_with("pai"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Repositório sem upstream: TODO commit conta como não enviado. É o caso do
    /// "código de identidade que só existe nesta máquina" — o pior, e o que mais
    /// precisa aparecer.
    #[test]
    fn sem_upstream_tudo_conta_como_nao_enviado() {
        let base = std::env::temp_dir().join(format!("gitrepo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let g = |args: &[&str]| {
            let mut v = vec!["-C", base.to_str().unwrap()];
            v.extend_from_slice(args);
            let _ = util::run("git", &v);
        };
        g(&["init", "-q"]);
        g(&["config", "user.email", "t@t"]);
        g(&["config", "user.name", "t"]);
        std::fs::write(base.join("a"), b"x").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "um"]);

        let e = estado_de(&base, "teste");
        assert_eq!(e.nao_enviados, 1, "sem upstream, o commit local não saiu daqui");
        assert!(e.remoto.is_none());
        assert!(!e.sujo);
        let _ = std::fs::remove_dir_all(&base);
    }
}
