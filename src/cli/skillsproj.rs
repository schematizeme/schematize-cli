//! `schematize skills applied` / `schematize skills rerun` — versão de skill POR PROJETO.
//!
//! A lógica mora em `schematize::skillsproj`; aqui é a face de linha de comando e o
//! disparo do agente que reaplica a skill.

use schematize::skillsproj::{self, Estado};
use schematize::{agentrun, registry, skills};
use std::path::{Path, PathBuf};

fn raiz() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))
}

/// Mostra o estado das skills NESTE projeto — ou registra uma como aplicada.
pub(crate) fn applied_cmd(mark: Option<String>) -> Result<(), String> {
    let root = raiz()?;
    if let Some(slug) = mark {
        // A versão registrada é a INSTALADA agora: é ela que o agente acabou de aplicar.
        let v = versao_instalada(&slug)
            .ok_or_else(|| format!("a skill '{slug}' não está instalada nesta máquina"))?;
        skillsproj::marcar(&root, &slug, &v)?;
        println!("registrado: '{slug}' v{v} aplicada neste projeto.");
        return Ok(());
    }

    let estados = skillsproj::estado_do_projeto(&root);
    if estados.is_empty() {
        println!("nenhuma skill instalada nesta máquina.");
        return Ok(());
    }
    let mut atrasadas = 0usize;
    println!("\x1b[1mSKILLS NESTE PROJETO\x1b[0m ({})", root.display());
    for (slug, e) in &estados {
        match e {
            Estado::Desatualizada { aplicada, instalada } => {
                atrasadas += 1;
                println!("  \x1b[33m↑\x1b[0m {slug:<16} aplicada v{aplicada} · instalada v{instalada}");
            }
            Estado::NuncaAplicada { instalada } => {
                println!("  \x1b[2m○\x1b[0m {slug:<16} nunca aplicada aqui · instalada v{instalada}");
            }
            Estado::Atual => println!("  \x1b[32m✓\x1b[0m {slug:<16} em dia"),
        }
    }
    if atrasadas > 0 {
        println!(
            "\n\x1b[33m{atrasadas} skill(s) evoluíram desde que moldaram este projeto.\x1b[0m"
        );
        println!("rodar de novo: schematize skills rerun");
    }
    Ok(())
}

/// Reaplica uma skill (ou todas as atrasadas) abrindo um agente no terminal.
pub(crate) fn rerun_cmd(slug: Option<String>) -> Result<(), String> {
    let root = raiz()?;
    let alvos: Vec<(String, String, String)> = match slug {
        Some(s) => {
            let inst = versao_instalada(&s)
                .ok_or_else(|| format!("a skill '{s}' não está instalada nesta máquina"))?;
            let atual = skillsproj::aplicadas(&root).get(&s).map(|a| a.versao.clone());
            vec![(s, atual.unwrap_or_else(|| "—".into()), inst)]
        }
        None => {
            let v = skillsproj::desatualizadas(&root);
            if v.is_empty() {
                println!("nenhuma skill desatualizada neste projeto.");
                return Ok(());
            }
            v
        }
    };

    println!("reaplicando {} skill(s) neste projeto:", alvos.len());
    for (s, de, para) in &alvos {
        println!("  {s}: v{de} → v{para}");
    }
    let prompt = skillsproj::prompt_rerun(&bin_atual(), &alvos);
    match agentrun::launch_prompt_in_terminal(&root, &prompt) {
        Ok(msg) => println!("{msg}"),
        // Sem terminal gráfico: entrega o prompt em vez de falhar.
        Err(e) => {
            println!("não consegui abrir um terminal ({e}). rode o agente você mesmo:\n");
            println!("--- prompt ---\n{prompt}\n--------------");
        }
    }
    Ok(())
}

/// Caminho do próprio binário, pro prompt mandar o agente chamar ESTE app.
fn bin_atual() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "schematize".into())
}

/// Versão instalada de uma skill pelo slug (`None` se não está na máquina).
fn versao_instalada(slug: &str) -> Option<String> {
    registry::catalog()
        .iter()
        .find(|i| i.slug == slug)
        .and_then(skills::installed_version)
}

/// Registra a aplicação de uma skill num `root` explícito (usado pela GUI).
#[allow(dead_code)]
pub(crate) fn _marcar_em(root: &Path, slug: &str) -> Result<(), String> {
    let v = versao_instalada(slug).ok_or_else(|| format!("'{slug}' não instalada"))?;
    skillsproj::marcar(root, slug, &v)
}
