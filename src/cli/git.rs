//! `schematize git` — contas, repositórios e o que ainda não saiu da máquina.

use crate::cli::args::*;
use schematize::gitcontas::{aplicar, contas::{self, Auth, Conta}, repos};
use schematize::{config, githist, util};
use std::path::PathBuf;

pub(crate) fn git_cmd(sub: GitCmd) -> Result<(), String> {
    match sub {
        GitCmd::Accounts => listar_contas(),
        GitCmd::Add { rotulo, usuario, email, chave, servico } => add_conta(rotulo, usuario, email, chave, servico),
        GitCmd::Remove { rotulo } => {
            if contas::remover(&rotulo)? {
                println!("conta '{rotulo}' removida.");
            } else {
                println!("não havia conta '{rotulo}'.");
            }
            Ok(())
        }
        GitCmd::Use { rotulo, remoto } => usar(rotulo, remoto),
        GitCmd::SshConfig { rotulo } => ssh_config(rotulo),
        GitCmd::Repos { rotulo, limite } => listar_repos(rotulo, limite),
        GitCmd::Status => status(),
        GitCmd::Log { limite } => log(limite),
    }
}

fn listar_contas() -> Result<(), String> {
    let v = contas::listar();
    if v.is_empty() {
        println!("nenhuma conta cadastrada.");
        println!("  schematize git add <rótulo> --usuario <login> --email <e-mail> [--chave <arquivo em ~/.ssh>]");
        return Ok(());
    }
    for c in v {
        let auth = match &c.auth {
            Auth::Ssh { chave } => {
                let ok = if aplicar::alias_configurado(&c) { "alias ok" } else { "\x1b[33mFALTA alias\x1b[0m" };
                format!("ssh:{chave} ({ok})")
            }
            Auth::Gh => "gh".to_string(),
        };
        println!("  \x1b[1m{:<12}\x1b[0m {} <{}>  {}  {auth}", c.rotulo, c.usuario, c.email, c.servico);
    }
    Ok(())
}

fn add_conta(rotulo: String, usuario: String, email: String, chave: Option<String>, servico: Option<String>) -> Result<(), String> {
    let c = Conta {
        rotulo: rotulo.clone(),
        usuario,
        email,
        servico: servico.unwrap_or_else(|| "github.com".into()),
        auth: match chave {
            Some(k) => Auth::Ssh { chave: k },
            None => Auth::Gh,
        },
    };
    contas::adicionar(c.clone())?;
    println!("conta '{rotulo}' cadastrada.");
    if matches!(c.auth, Auth::Ssh { .. }) && !aplicar::alias_configurado(&c) {
        println!("falta o alias SSH — rode: schematize git ssh-config {rotulo}");
    }
    Ok(())
}

fn ssh_config(rotulo: String) -> Result<(), String> {
    let c = contas::por_rotulo(&rotulo).ok_or_else(|| format!("conta '{rotulo}' não existe"))?;
    if aplicar::escreve_alias(&c)? {
        println!("alias adicionado ao ~/.ssh/config:\n{}", c.bloco_ssh_config());
    } else {
        println!("nada a fazer (conta `gh` ou alias já configurado).");
    }
    Ok(())
}

fn usar(rotulo: String, remoto: Option<String>) -> Result<(), String> {
    let c = contas::por_rotulo(&rotulo).ok_or_else(|| format!("conta '{rotulo}' não existe"))?;
    let raiz = std::env::current_dir().map_err(|e| e.to_string())?;
    let feitos = aplicar::aplicar(&raiz, &c, remoto.as_deref().unwrap_or("origin"))?;
    println!("repositório {} agora usa a conta '{rotulo}':", raiz.display());
    for f in feitos {
        println!("  {f}");
    }
    Ok(())
}

fn listar_repos(rotulo: Option<String>, limite: usize) -> Result<(), String> {
    let cs = match rotulo {
        Some(r) => vec![contas::por_rotulo(&r).ok_or_else(|| format!("conta '{r}' não existe"))?],
        None => contas::listar(),
    };
    if cs.is_empty() {
        return Err("nenhuma conta cadastrada — `schematize git add`".into());
    }
    for c in cs {
        println!("\n\x1b[1m{}\x1b[0m ({}):", c.rotulo, c.usuario);
        match repos::listar(&c, limite) {
            Ok(rs) => {
                for r in rs {
                    let vis = if r.privado { "privado" } else { "público" };
                    println!("  {:<40} {:<8} {}  {}", r.caminho, vis, r.atualizado, r.descricao);
                }
            }
            Err(e) => println!("  \x1b[33m{e}\x1b[0m"),
        }
    }
    Ok(())
}

/// O que ainda NÃO saiu da máquina — a pergunta que git não responde sozinho.
fn status() -> Result<(), String> {
    let estados = repos::estado_dos_projetos(&config::dev_dirs());
    if estados.is_empty() {
        println!("nenhum projeto git nos diretórios de dev.");
        return Ok(());
    }
    println!("{:<26} {:<12} {:>10}  {}", "PROJETO", "CONTA", "NÃO ENVIADOS", "ESTADO");
    let mut risco = 0usize;
    for e in &estados {
        let conta = e.conta.clone().unwrap_or_else(|| format!("? {}", e.email));
        let sujo = if e.sujo { "sujo" } else { "" };
        let cor = if e.nao_enviados > 0 { "\x1b[33m" } else { "" };
        println!("{:<26} {:<12} {cor}{:>10}\x1b[0m  {sujo}", e.nome, conta, e.nao_enviados);
        if e.nao_enviados > 0 {
            risco += 1;
        }
    }
    if risco > 0 {
        println!("\n\x1b[33m{risco} projeto(s) com commit que só existe nesta máquina.\x1b[0m");
    }
    Ok(())
}

/// Commits do projeto atual, marcando os que já foram enviados.
fn log(limite: usize) -> Result<(), String> {
    let raiz = std::env::current_dir().map_err(|e| e.to_string())?;
    let up = githist::upstream(&raiz);
    if let Some(u) = &up {
        let remoto = u.remote.clone().unwrap_or_else(|| "(sem upstream)".into());
        println!("{} → {}  (+{} / -{})", u.branch, remoto, u.ahead, u.behind);
    }
    for c in githist::commits(&raiz, limite) {
        let marca = if c.pushed { "\x1b[32m✓\x1b[0m" } else { "\x1b[33m↑\x1b[0m" };
        println!("{marca} {} {} {:<14} {}", c.short, c.date, elide(&c.author, 14), c.subject);
    }
    println!("\n\x1b[32m✓\x1b[0m já enviado   \x1b[33m↑\x1b[0m só nesta máquina");
    Ok(())
}

/// Corta um texto no comprimento dado (seguro a UTF-8).
fn elide(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
}

/// Reexporta o home pra quem monta caminho de chave na UI.
pub(crate) fn _home() -> PathBuf {
    util::home()
}
