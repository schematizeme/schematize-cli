//! AUTO-DETECÇÃO de contas de git/GitHub já presentes na máquina.
//!
//! O quê: varre as quatro fontes que sabem quem você é — `gh auth status`, o
//! `git config --global`, o `~/.ssh/` (chaves + `config`) e os e-mails configurados
//! REPO A REPO — e devolve sugestões de conta prontas pra cadastrar.
//! Onde: `schematize git detect` (CLI) e o botão da aba de contas (GUI).
//!
//! ## Por que existe
//! Cadastrar conta à mão é onde a pessoa erra: digita o e-mail com typo, aponta pra uma
//! chave que não existe, ou cria um rótulo que não bate com nada. Tudo isso já está no
//! disco — perguntar de novo é fazer o usuário provar o que a máquina sabe (§48).
//!
//! ## Postura
//! Isto **sugere**, nunca grava sozinho: cada sugestão diz DE ONDE veio, e quem decide é
//! quem lê. Uma conta cadastrada errada empurra commit com a identidade errada, que é o
//! tipo de estrago que ninguém percebe na hora.

use super::contas::{Auth, Conta};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// De onde uma sugestão veio — é o que permite ao usuário julgar se confia nela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origem {
    /// Sessão ativa do `gh` CLI.
    Gh,
    /// `git config --global user.name/user.email`.
    GitGlobal,
    /// Par de chaves em `~/.ssh` + bloco no `~/.ssh/config`.
    SshConfig,
    /// `git config user.email` dentro de um repositório clonado.
    Repo(PathBuf),
}

impl Origem {
    /// Frase curta pra listagem — o usuário decide confiando nisto.
    pub fn descricao(&self) -> String {
        match self {
            Origem::Gh => "sessão do `gh` CLI".into(),
            Origem::GitGlobal => "git config --global".into(),
            Origem::SshConfig => "~/.ssh/config".into(),
            Origem::Repo(p) => format!("repo {}", p.display()),
        }
    }
}

/// Uma conta CANDIDATA, com a procedência.
#[derive(Debug, Clone)]
pub struct Sugestao {
    pub conta: Conta,
    pub origem: Origem,
    /// `true` quando já existe conta cadastrada com o mesmo usuário+serviço.
    pub ja_cadastrada: bool,
}

/// Lê o login e o host de `gh auth status`.
///
/// O quê: extrai `(host, usuário)` de cada linha "Logged in to <host> account <user>".
/// Onde: [`detectar`]. PURA — o teste passa a saída capturada, sem executar o `gh`.
///
/// Formato real (gh 2.x):
/// ```text
/// github.com
///   ✓ Logged in to github.com account Lucassa02 (/home/tom/.config/gh/hosts.yml)
/// ```
/// **Entrada:** stdout+stderr do `gh auth status`. **Saída:** pares `(host, usuário)`.
/// **Efeitos:** nenhum.
pub fn parse_gh_status(saida: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in saida.lines() {
        let l = l.trim();
        let Some(resto) = l.split_once("Logged in to ").map(|(_, r)| r) else {
            continue;
        };
        let mut it = resto.split_whitespace();
        let (Some(host), Some(rotulo_account), Some(user)) = (it.next(), it.next(), it.next())
        else {
            continue;
        };
        // `account` é literal do gh; se mudar de formato, preferimos NÃO adivinhar.
        if rotulo_account != "account" {
            continue;
        }
        out.push((host.to_string(), user.to_string()));
    }
    out
}

/// Lê os blocos `Host … / User … / IdentityFile …` de um `~/.ssh/config`.
///
/// O quê: devolve `(host, arquivo-da-chave)` por bloco que tenha `IdentityFile`.
/// Onde: [`detectar`]. PURA — recebe o texto, não o caminho.
///
/// Por que só o nome do arquivo: é o que a `Conta` guarda (`Auth::Ssh { chave }`), e
/// caminho absoluto no cadastro quebra quando o HOME muda.
/// **Entrada:** conteúdo do config. **Saída:** pares `(host, chave)`.
/// **Efeitos:** nenhum.
pub fn parse_ssh_config(texto: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut host: Option<String> = None;
    for linha in texto.lines() {
        let l = linha.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let (chave, valor) = match l.split_once(char::is_whitespace) {
            Some((k, v)) => (k.to_ascii_lowercase(), v.trim()),
            None => continue,
        };
        match chave.as_str() {
            "host" => host = Some(valor.to_string()),
            "identityfile" => {
                if let Some(h) = &host {
                    let arquivo = valor.rsplit('/').next().unwrap_or(valor);
                    out.push((h.clone(), arquivo.to_string()));
                }
            }
            _ => {}
        }
    }
    out
}

/// Deriva um rótulo curto, único e sem espaço a partir do usuário/host.
///
/// O quê: minúsculas, só `[a-z0-9-]`, e sufixo numérico se colidir com os já usados.
/// Onde: [`detectar`], ao montar cada sugestão.
/// Por que: o rótulo é a CHAVE do cadastro; deixar o usuário inventar na hora é onde
/// nascem `pessoal`, `Pessoal ` e `pessoal2` apontando pra mesma coisa.
/// **Entrada:** usuário e os rótulos já tomados. **Saída:** rótulo livre.
pub fn rotulo_livre(usuario: &str, tomados: &[String]) -> String {
    let base: String = usuario
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let base = base.trim_matches('-').to_string();
    let base = if base.is_empty() { "conta".to_string() } else { base };
    if !tomados.iter().any(|t| t == &base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|c| !tomados.iter().any(|t| t == c)).unwrap_or(base)
}

/// `git config --global <chave>`, ou `None` se ausente/ilegível.
fn git_global(chave: &str) -> Option<String> {
    let v = crate::util::run("git", &["config", "--global", chave]).ok()?;
    let v = v.trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// `git config user.email` DENTRO de um repo, ou `None` se não houver override local.
fn git_email_do_repo(repo: &Path) -> Option<String> {
    let p = repo.to_str()?;
    let v = crate::util::run("git", &["-C", p, "config", "--local", "user.email"]).ok()?;
    let v = v.trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// Chaves privadas em `~/.ssh` (ignora `.pub`, `known_hosts`, `config` e revogadas).
fn chaves_ssh() -> Vec<String> {
    let dir = crate::util::home().join(".ssh");
    let mut v: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let nome = e.file_name().to_str()?.to_string();
            let ignorar = nome.ends_with(".pub")
                || nome.contains("REVOGADA")
                || matches!(nome.as_str(), "config" | "known_hosts" | "authorized_keys")
                || nome.starts_with('.');
            (!ignorar && e.path().is_file()).then_some(nome)
        })
        .collect();
    v.sort();
    v
}

/// Varre a máquina e devolve as contas CANDIDATAS, sem gravar nada.
///
/// O quê: junta `gh auth status`, `git config --global`, `~/.ssh/config` + chaves e os
/// e-mails locais dos repos conhecidos, e monta uma [`Sugestao`] por identidade distinta.
/// Onde: `schematize git detect` e a GUI.
///
/// ## Como resolve o e-mail
/// O `gh` sabe o LOGIN mas não o e-mail de commit; o `git config` sabe o E-MAIL mas não o
/// login. A junção é por isso: login do `gh` + e-mail global, e os repos entram quando têm
/// override local (é exatamente o caso de quem separa trabalho e pessoal por repositório).
///
/// ## Dedupe
/// A chave é `usuario@servico`. A primeira fonte a aparecer ganha, na ordem `gh` →
/// ssh-config → global → repos: da mais específica (sessão real) pra mais genérica.
///
/// **Entrada:** `repos` — diretórios a inspecionar por e-mail local (pode ser vazio).
/// **Saída:** sugestões, cada uma com origem e se já está cadastrada.
/// **Efeitos:** executa `gh`/`git` e lê `~/.ssh` — só leitura, nunca grava.
pub fn detectar(repos: &[PathBuf]) -> Vec<Sugestao> {
    let existentes = super::contas::listar();
    let ja = |u: &str, s: &str| existentes.iter().any(|c| c.usuario == u && c.servico == s);

    let email_global = git_global("user.email");
    let nome_global = git_global("user.name");
    let ssh = std::fs::read_to_string(crate::util::home().join(".ssh/config")).unwrap_or_default();
    let blocos = parse_ssh_config(&ssh);
    let chaves = chaves_ssh();

    // usuario@servico -> (Sugestao)
    let mut achadas: BTreeMap<String, Sugestao> = BTreeMap::new();
    let mut rotulos: Vec<String> = existentes.iter().map(|c| c.rotulo.clone()).collect();

    let push = |achadas: &mut BTreeMap<String, Sugestao>,
                rotulos: &mut Vec<String>,
                usuario: String,
                email: String,
                servico: String,
                auth: Auth,
                origem: Origem| {
        let k = format!("{usuario}@{servico}");
        if achadas.contains_key(&k) {
            return; // a fonte mais específica já ganhou
        }
        let rotulo = rotulo_livre(&usuario, rotulos);
        rotulos.push(rotulo.clone());
        let ja_cadastrada = ja(&usuario, &servico);
        achadas.insert(
            k,
            Sugestao {
                conta: Conta { rotulo, usuario, email, servico, auth },
                origem,
                ja_cadastrada,
            },
        );
    };

    // 1) `gh` — a fonte mais confiável: é uma sessão que existe de verdade.
    let saida = crate::util::run("gh", &["auth", "status"]).unwrap_or_default();
    for (host, user) in parse_gh_status(&saida) {
        push(
            &mut achadas,
            &mut rotulos,
            user,
            email_global.clone().unwrap_or_default(),
            host,
            Auth::Gh,
            Origem::Gh,
        );
    }

    // 2) `~/.ssh/config` — cada bloco com IdentityFile vira candidata SSH. O `Host` costuma
    //    ser o alias por conta (`github.com-pessoal`), de onde sai o usuário provável.
    for (host, chave) in blocos {
        let servico = host.split('-').next().unwrap_or(&host).to_string();
        let usuario = host
            .split_once('-')
            .map(|(_, u)| u.to_string())
            .unwrap_or_else(|| nome_global.clone().unwrap_or_else(|| "desconhecido".into()));
        push(
            &mut achadas,
            &mut rotulos,
            usuario,
            email_global.clone().unwrap_or_default(),
            servico,
            Auth::Ssh { chave },
            Origem::SshConfig,
        );
    }

    // 3) `git config --global` — só entra se ainda não achamos ninguém: é a identidade
    //    genérica da máquina, sem login de serviço associado.
    if achadas.is_empty() {
        if let (Some(nome), Some(email)) = (nome_global.clone(), email_global.clone()) {
            let auth = match chaves.first() {
                Some(k) => Auth::Ssh { chave: k.clone() },
                None => Auth::Gh,
            };
            push(
                &mut achadas,
                &mut rotulos,
                nome,
                email,
                "github.com".into(),
                auth,
                Origem::GitGlobal,
            );
        }
    }

    // 4) Repos com e-mail LOCAL diferente do global — é o sinal de identidade separada.
    for r in repos {
        let Some(email) = git_email_do_repo(r) else { continue };
        if Some(&email) == email_global.as_ref() {
            continue; // igual ao global: não é identidade separada, é herança
        }
        let usuario = email.split('@').next().unwrap_or(&email).to_string();
        push(
            &mut achadas,
            &mut rotulos,
            usuario,
            email,
            "github.com".into(),
            Auth::Gh,
            Origem::Repo(r.clone()),
        );
    }

    achadas.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O QUE: o formato REAL do `gh auth status` (gh 2.x) é lido, e linha que não casa o
    /// formato é IGNORADA em vez de adivinhada.
    ///
    /// POR QUE não adivinhar: cadastrar a conta errada empurra commit com identidade errada
    /// — estrago que ninguém percebe na hora. Se o `gh` mudar a saída, preferimos não
    /// sugerir nada a sugerir lixo.
    #[test]
    fn le_o_gh_auth_status_real() {
        let saida = "github.com\n  \
            ✓ Logged in to github.com account Lucassa02 (/home/tom/.config/gh/hosts.yml)\n  \
            - Active account: true\n  \
            - Git operations protocol: https\n";
        assert_eq!(parse_gh_status(saida), vec![("github.com".into(), "Lucassa02".into())]);

        // Duas contas (multi-host) — o gh lista uma por linha.
        let duas = "  ✓ Logged in to github.com account alice (x)\n  \
                    ✓ Logged in to gitlab.com account bob (y)\n";
        assert_eq!(
            parse_gh_status(duas),
            vec![("github.com".into(), "alice".into()), ("gitlab.com".into(), "bob".into())]
        );

        // Ruído e formato inesperado não viram conta.
        assert!(parse_gh_status("You are not logged into any GitHub hosts.").is_empty());
        assert!(parse_gh_status("Logged in to github.com como alguem").is_empty());
    }

    /// O QUE: blocos do `~/.ssh/config` viram (host, arquivo-da-chave), com o caminho
    /// reduzido ao NOME — caminho absoluto no cadastro quebra quando o HOME muda.
    #[test]
    fn le_blocos_do_ssh_config() {
        let cfg = "# comentario\n\
                   Host github.com-pessoal\n  \
                     HostName github.com\n  \
                     IdentityFile ~/.ssh/id_pessoal\n\
                   \n\
                   Host github.com-trabalho\n  \
                     IdentityFile /home/x/.ssh/id_trab\n\
                   \n\
                   Host sem-chave\n  \
                     HostName exemplo.com\n";
        assert_eq!(
            parse_ssh_config(cfg),
            vec![
                ("github.com-pessoal".into(), "id_pessoal".into()),
                ("github.com-trabalho".into(), "id_trab".into()),
            ],
            "bloco sem IdentityFile não vira candidata"
        );
    }

    /// O QUE: o rótulo derivado é sempre único e sem caractere que quebre o cadastro.
    ///
    /// POR QUE: o rótulo é a CHAVE. Sem isto nascem `pessoal`, `Pessoal ` e `pessoal2`
    /// apontando pra mesma identidade.
    #[test]
    fn rotulo_e_unico_e_limpo() {
        assert_eq!(rotulo_livre("Lucassa02", &[]), "lucassa02");
        assert_eq!(rotulo_livre("nome com espaço", &[]), "nome-com-espa-o");
        assert_eq!(rotulo_livre("alice", &["alice".into()]), "alice-2");
        assert_eq!(rotulo_livre("alice", &["alice".into(), "alice-2".into()]), "alice-3");
        assert_eq!(rotulo_livre("!!!", &[]), "conta", "sem nada aproveitável, nome neutro");
    }
}
