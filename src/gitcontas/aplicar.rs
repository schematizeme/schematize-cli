//! APLICAR uma conta a um repositório — a operação que evita o commit com identidade errada.
//!
//! O quê: escreve no repo a identidade (`user.name`/`user.email` LOCAIS) e reaponta o
//! remoto pro host da conta. Onde: `schematize git usar <conta>` e o botão da GUI.
//!
//! Por que local e não global: a config global é a que faz o commit sair com a
//! identidade errada quando se troca de projeto. Local, cada repo carrega a sua e não
//! há o que esquecer.

use super::contas::{Auth, Conta};
use crate::util;
use std::path::Path;

/// Roda `git -C <root> <args>`.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut v = vec!["-C", root.to_str().unwrap_or(".")];
    v.extend_from_slice(args);
    util::run("git", &v)
}

/// `usuario/repo` a partir de uma URL de remoto (SSH ou HTTPS, com ou sem `.git`).
///
/// PURA e testada: é o que permite reapontar o remoto pra outra conta sem perder de
/// vista QUAL repositório é.
pub fn caminho_do_repo(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches(".git");
    // git@host:dono/repo  |  ssh://git@host/dono/repo
    let resto = if let Some((_, r)) = u.split_once(':').filter(|(a, _)| !a.contains("//")) {
        r.to_string()
    } else {
        // https://host/dono/repo
        let sem_esquema = u.split_once("://").map(|(_, r)| r).unwrap_or(u);
        sem_esquema.split_once('/').map(|(_, r)| r.to_string())?
    };
    let partes: Vec<&str> = resto.trim_matches('/').split('/').collect();
    if partes.len() < 2 || partes.iter().any(|p| p.is_empty()) {
        return None;
    }
    Some(format!("{}/{}", partes[partes.len() - 2], partes[partes.len() - 1]))
}

/// Qual conta cadastrada está em uso neste repo? Compara o e-mail local do git.
pub fn conta_em_uso(root: &Path) -> Option<Conta> {
    let email = git(root, &["config", "--local", "user.email"]).ok()?;
    let email = email.trim();
    super::contas::listar().into_iter().find(|c| c.email == email)
}

/// Aplica a conta ao repositório: identidade local + remoto apontando pro host dela.
///
/// Não mexe em config global e não toca em nada além de `user.name`, `user.email` e a
/// URL do remoto. Devolve o que mudou, pra o chamador poder mostrar.
pub fn aplicar(root: &Path, c: &Conta, remoto: &str) -> Result<Vec<String>, String> {
    if !root.join(".git").exists() {
        return Err(format!("{} não é um repositório git", root.display()));
    }
    let mut feitos = Vec::new();

    git(root, &["config", "--local", "user.name", &c.usuario])?;
    git(root, &["config", "--local", "user.email", &c.email])?;
    feitos.push(format!("identidade local: {} <{}>", c.usuario, c.email));

    // Reaponta o remoto SÓ se der pra saber qual repositório é — nunca inventamos URL.
    if let Ok(url_atual) = git(root, &["remote", "get-url", remoto]) {
        if let Some(caminho) = caminho_do_repo(url_atual.trim()) {
            let nova = c.url_remoto(&caminho);
            if nova != url_atual.trim() {
                git(root, &["remote", "set-url", remoto, &nova])?;
                feitos.push(format!("remoto {remoto}: {nova}"));
            }
        } else {
            feitos.push(format!("remoto {remoto}: não reconheci a URL, deixei como está"));
        }
    }

    if let Auth::Ssh { .. } = c.auth {
        if !alias_configurado(c) {
            feitos.push(format!(
                "FALTA o alias SSH — rode `schematize git ssh-config {}` (ou cole o bloco em ~/.ssh/config)",
                c.rotulo
            ));
        }
    }
    Ok(feitos)
}

/// O alias da conta já está no `~/.ssh/config`?
pub fn alias_configurado(c: &Conta) -> bool {
    let Ok(s) = std::fs::read_to_string(util::home().join(".ssh").join("config")) else {
        return false;
    };
    let alvo = format!("Host {}", c.host_do_remoto());
    s.lines().any(|l| l.trim() == alvo)
}

/// Acrescenta o bloco do alias ao `~/.ssh/config` (idempotente).
pub fn escreve_alias(c: &Conta) -> Result<bool, String> {
    let bloco = c.bloco_ssh_config();
    if bloco.is_empty() || alias_configurado(c) {
        return Ok(false);
    }
    let dir = util::home().join(".ssh");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let p = dir.join("config");
    let atual = std::fs::read_to_string(&p).unwrap_or_default();
    let novo = if atual.is_empty() { bloco } else { format!("{}\n{}", atual.trim_end(), bloco) };
    std::fs::write(&p, novo).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O parser cobre as formas de URL que aparecem na vida real — inclusive com
    /// alias por conta (`github.com-pessoal`), que é o que a ferramenta grava.
    #[test]
    fn extrai_dono_e_repo_de_qualquer_url() {
        for (url, esperado) in [
            ("git@github.com:org/repo.git", "org/repo"),
            ("git@github.com-pessoal:org/repo.git", "org/repo"),
            ("https://github.com/org/repo.git", "org/repo"),
            ("https://github.com/org/repo", "org/repo"),
            ("ssh://git@github.com/org/repo.git", "org/repo"),
            ("git@gitlab.com:grupo/sub/repo.git", "sub/repo"),
        ] {
            assert_eq!(caminho_do_repo(url).as_deref(), Some(esperado), "url: {url}");
        }
    }

    /// URL que não dá pra entender vira `None` — e o `aplicar` então NÃO mexe no
    /// remoto. Inventar URL seria pior que não fazer nada.
    #[test]
    fn url_incompreensivel_nao_vira_palpite() {
        assert_eq!(caminho_do_repo("sei-la"), None);
        assert_eq!(caminho_do_repo(""), None);
        assert_eq!(caminho_do_repo("https://github.com/"), None);
    }

    /// Fora de um repo git, `aplicar` recusa em vez de escrever config solta.
    #[test]
    fn recusa_fora_de_repo_git() {
        let base = std::env::temp_dir().join(format!("git-nao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let c = Conta {
            rotulo: "x".into(), usuario: "u".into(), email: "e@x".into(),
            servico: "github.com".into(), auth: Auth::Gh,
        };
        assert!(aplicar(&base, &c, "origin").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
