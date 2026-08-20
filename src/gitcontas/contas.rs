//! CONTAS de git/GitHub cadastradas — quem faz push pra onde.
//!
//! O quê: a lista de contas da máquina (rótulo, usuário, e-mail, como autentica) e
//! qual delas vale em cada repositório. Onde: `schematize git` e a tela Git da GUI.
//!
//! O problema: quem trabalha com mais de uma conta (pessoal, empresa, cliente) empurra
//! commit com a identidade errada — e no GitHub isso é público e não se apaga. Pior
//! quando a chave SSH também é a errada: o push simplesmente falha, ou vai pra conta
//! errada. A correção é sempre a mesma dupla — `user.name`/`user.email` LOCAIS do repo
//! e a chave certa —, e é isso que fica registrado aqui pra ser aplicado num comando.
//!
//! Segredo NÃO mora aqui: guardamos o NOME da chave SSH e o host, nunca a chave nem
//! token. Autenticação continua no `gh`/agente SSH, que é quem sabe guardar isso.

use crate::util;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Como a conta autentica no push.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Auth {
    /// Chave SSH (nome do arquivo em `~/.ssh`, sem caminho).
    Ssh { chave: String },
    /// `gh` CLI (token guardado por ele).
    Gh,
}

/// Uma conta cadastrada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conta {
    /// Rótulo curto e único ("pessoal", "volucer", "cliente-x").
    pub rotulo: String,
    /// Usuário no serviço (login do GitHub).
    pub usuario: String,
    /// E-mail que vai no commit.
    pub email: String,
    /// Serviço ("github.com" por padrão; outro host pra GitLab/Gitea self-hosted).
    #[serde(default = "github")]
    pub servico: String,
    pub auth: Auth,
}

fn github() -> String {
    "github.com".to_string()
}

impl Conta {
    /// Host a usar na URL do remoto.
    ///
    /// Com SSH, usamos um ALIAS por conta (`github.com-pessoal`) em vez do host real.
    /// É o truque que faz duas contas conviverem: o `~/.ssh/config` mapeia cada alias
    /// pra a chave certa, então o remoto carrega consigo QUAL identidade usar — em vez
    /// de depender do agente adivinhar (que é como se empurra com a conta errada).
    pub fn host_do_remoto(&self) -> String {
        match &self.auth {
            Auth::Ssh { .. } => format!("{}-{}", self.servico, self.rotulo),
            Auth::Gh => self.servico.clone(),
        }
    }

    /// URL de remoto pra `usuario/repo` com esta conta.
    pub fn url_remoto(&self, repo: &str) -> String {
        match &self.auth {
            Auth::Ssh { .. } => format!("git@{}:{}.git", self.host_do_remoto(), repo),
            Auth::Gh => format!("https://{}/{}.git", self.servico, repo),
        }
    }

    /// O bloco de `~/.ssh/config` que faz o alias funcionar. Vazio pra contas `gh`.
    pub fn bloco_ssh_config(&self) -> String {
        let Auth::Ssh { chave } = &self.auth else {
            return String::new();
        };
        format!(
            "# schematize: conta '{}'\nHost {}\n  HostName {}\n  User git\n  IdentityFile ~/.ssh/{}\n  IdentitiesOnly yes\n",
            self.rotulo,
            self.host_do_remoto(),
            self.servico,
            chave
        )
    }
}

/// Arquivo onde as contas moram.
pub fn arquivo() -> PathBuf {
    util::claude_dir().join("schematize").join("contas.json")
}

/// Lê as contas cadastradas (vazio se não houver / arquivo inválido).
pub fn listar() -> Vec<Conta> {
    let Ok(s) = std::fs::read_to_string(arquivo()) else {
        return Vec::new();
    };
    serde_json::from_str(&s).unwrap_or_default()
}

/// Grava a lista inteira.
pub fn gravar(v: &[Conta]) -> Result<(), String> {
    let p = arquivo();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(&p, body).map_err(|e| e.to_string())
}

/// Adiciona (ou substitui, pelo rótulo) uma conta.
pub fn adicionar(c: Conta) -> Result<(), String> {
    if c.rotulo.trim().is_empty() {
        return Err("rótulo vazio".into());
    }
    if c.rotulo.contains(char::is_whitespace) {
        return Err("rótulo não pode ter espaço (ele vira parte do host SSH)".into());
    }
    let mut v = listar();
    v.retain(|x| x.rotulo != c.rotulo);
    v.push(c);
    gravar(&v)
}

/// Remove pelo rótulo. `Ok(false)` se não havia.
pub fn remover(rotulo: &str) -> Result<bool, String> {
    let mut v = listar();
    let antes = v.len();
    v.retain(|x| x.rotulo != rotulo);
    let mexeu = v.len() != antes;
    gravar(&v)?;
    Ok(mexeu)
}

/// Busca pelo rótulo.
pub fn por_rotulo(rotulo: &str) -> Option<Conta> {
    listar().into_iter().find(|c| c.rotulo == rotulo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conta_ssh() -> Conta {
        Conta {
            rotulo: "pessoal".into(),
            usuario: "fulano".into(),
            email: "fulano@exemplo.com".into(),
            servico: github(),
            auth: Auth::Ssh { chave: "id_pessoal".into() },
        }
    }

    /// O ALIAS por conta é o que faz duas identidades conviverem: o remoto carrega
    /// consigo qual chave usar, em vez de depender do agente adivinhar.
    #[test]
    fn ssh_usa_alias_por_conta() {
        let c = conta_ssh();
        assert_eq!(c.host_do_remoto(), "github.com-pessoal");
        assert_eq!(c.url_remoto("org/repo"), "git@github.com-pessoal:org/repo.git");
        let bloco = c.bloco_ssh_config();
        assert!(bloco.contains("Host github.com-pessoal"));
        assert!(bloco.contains("HostName github.com"), "o alias resolve pro host real");
        assert!(bloco.contains("IdentityFile ~/.ssh/id_pessoal"));
        assert!(bloco.contains("IdentitiesOnly yes"), "sem isto o agente ainda pode oferecer outra chave");
    }

    /// Conta via `gh` usa HTTPS e o host real — quem guarda o token é o `gh`.
    #[test]
    fn gh_usa_https_sem_alias() {
        let c = Conta { auth: Auth::Gh, ..conta_ssh() };
        assert_eq!(c.host_do_remoto(), "github.com");
        assert_eq!(c.url_remoto("org/repo"), "https://github.com/org/repo.git");
        assert!(c.bloco_ssh_config().is_empty());
    }

    /// Rótulo vira parte do host SSH — espaço ali gera config quebrada.
    #[test]
    fn rotulo_nao_aceita_espaco() {
        let c = Conta { rotulo: "minha conta".into(), ..conta_ssh() };
        assert!(adicionar(c).is_err());
    }

    /// Serviço diferente (GitLab, Gitea) é suportado sem caso especial.
    #[test]
    fn outro_servico_funciona_igual() {
        let c = Conta { servico: "gitlab.com".into(), ..conta_ssh() };
        assert_eq!(c.url_remoto("g/r"), "git@gitlab.com-pessoal:g/r.git");
        assert!(c.bloco_ssh_config().contains("HostName gitlab.com"));
    }

    /// Nenhuma forma de segredo é serializada — só nome de chave e host.
    #[test]
    fn nao_guarda_segredo() {
        let json = serde_json::to_string(&conta_ssh()).unwrap();
        assert!(json.contains("id_pessoal"), "guarda o NOME da chave");
        assert!(!json.to_lowercase().contains("private"));
        assert!(!json.contains("BEGIN"), "nunca o conteúdo de uma chave");
        assert!(!json.to_lowercase().contains("token"));
    }
}
