//! `schematize disco` — inventário e limpeza do lixo recriável.
//!
//! Lista por DISCO (a pergunta real é "o que está enchendo ESTE disco"), por tipo, e
//! os maiores achados. Só apaga com `--limpar`, e nunca sem dizer antes o que vai
//! apagar. A poda de volumes do Docker fica fora de qualquer limpeza em lote.

use crate::cli::args::*;
use schematize::disco::{self, docker, tamanho::legivel};
use schematize::{config, i18n::t};
use std::io::Write;

/// Corte de ruído: abaixo disto não vale nem listar.
const MINIMO: u64 = 50 * 1024 * 1024;

pub(crate) fn disco_cmd(sub: DiscoCmd) -> Result<(), String> {
    match sub {
        DiscoCmd::List { min_dias } => listar(min_dias),
        DiscoCmd::Clean { min_dias, tipo, montagem, yes } => limpar(min_dias, tipo, montagem, yes),
        DiscoCmd::Docker { podar, yes } => docker_cmd(podar, yes),
    }
}

/// Varre e imprime o inventário — três cortes da mesma informação.
fn listar(min_dias: u64) -> Result<(), String> {
    let devs = config::dev_dirs();
    if devs.is_empty() {
        println!("nenhum diretório de dev cadastrado — use `schematize projects add <dir>`.");
    }
    println!("varrendo {} diretório(s) de dev…", devs.len());
    let todos = disco::inventario(&devs, MINIMO);
    let achados: Vec<_> = todos.into_iter().filter(|a| a.dias_parado >= min_dias).collect();

    println!("\n\x1b[1mPOR DISCO\x1b[0m (é o que enche)");
    for (m, b) in disco::por_montagem(&achados) {
        println!("  {:>10}  {}", legivel(b), m.display());
    }

    println!("\n\x1b[1mPOR TIPO\x1b[0m");
    for (t, b) in disco::por_tipo(&achados) {
        let custo = if t.custa_rede() { "baixa de novo" } else { "compila de novo" };
        println!("  {:>10}  {:<16} ({custo})", legivel(b), t.rotulo());
    }

    println!("\n\x1b[1mMAIORES\x1b[0m");
    for a in achados.iter().take(15) {
        println!("  {:>10}  {:<16} {} dias  {}", legivel(a.bytes), a.tipo.rotulo(), a.dias_parado, a.caminho.display());
    }

    let total: u64 = achados.iter().map(|a| a.bytes).sum();
    println!("\ntotal recuperável nos projetos: \x1b[1m{}\x1b[0m", legivel(total));
    imprime_docker();
    println!("\nlimpar: `schematize disco clean --min-dias 30`   (docker: `schematize disco docker`)");
    Ok(())
}

/// Bloco do Docker no inventário. Silencioso se o docker não estiver disponível —
/// máquina sem docker não deve ver erro de docker.
fn imprime_docker() {
    let uso = docker::uso();
    if uso.is_empty() {
        return;
    }
    println!("\n\x1b[1mDOCKER\x1b[0m");
    let mut rec = 0u64;
    for c in &uso {
        println!("  {:>10}  {:<16} (recuperável: {})", legivel(c.bytes), c.tipo, legivel(c.recuperavel));
        rec += c.recuperavel;
    }
    println!("  recuperável no docker: \x1b[1m{}\x1b[0m", legivel(rec));
}

/// Apaga os achados que casam com os filtros. Mostra a lista ANTES e pede confirmação.
fn limpar(min_dias: u64, tipo: Option<String>, montagem: Option<String>, yes: bool) -> Result<(), String> {
    let devs = config::dev_dirs();
    let alvos: Vec<_> = disco::inventario(&devs, MINIMO)
        .into_iter()
        .filter(|a| a.dias_parado >= min_dias)
        .filter(|a| tipo.as_deref().is_none_or(|t| a.tipo.rotulo().contains(t)))
        .filter(|a| montagem.as_deref().is_none_or(|m| a.montagem.starts_with(m)))
        .collect();

    if alvos.is_empty() {
        println!("nada a limpar com esses filtros.");
        return Ok(());
    }
    let total: u64 = alvos.iter().map(|a| a.bytes).sum();
    println!("vou apagar {} item(ns), liberando ~{}:", alvos.len(), legivel(total));
    for a in &alvos {
        println!("  {:>10}  {:<16} {}  \x1b[2m({})\x1b[0m", legivel(a.bytes), a.tipo.rotulo(), a.caminho.display(), a.refaz);
    }
    if !yes && !confirma("apagar? [s/N] ") {
        println!("cancelado.");
        return Ok(());
    }
    let mut liberado = 0u64;
    for a in &alvos {
        match disco::remover(a, &devs) {
            Ok(b) => liberado += b,
            Err(e) => println!("  \x1b[33mfalhou\x1b[0m {e}"),
        }
    }
    println!("liberado: \x1b[1m{}\x1b[0m", legivel(liberado));
    Ok(())
}

/// `schematize disco docker` — lista as podas; com `--podar <rótulo>`, executa uma.
fn docker_cmd(podar: Option<String>, yes: bool) -> Result<(), String> {
    if !docker::disponivel() {
        return Err("docker não está disponível (não instalado ou o daemon está parado).".into());
    }
    let Some(rotulo) = podar else {
        imprime_docker();
        println!("\npodas disponíveis:");
        for (r, _, destrutiva) in docker::podas() {
            let marca = if destrutiva { "  \x1b[31m← apaga DADOS\x1b[0m" } else { "" };
            println!("  schematize disco docker --podar \"{r}\"{marca}");
        }
        return Ok(());
    };
    let destrutiva = docker::podas().into_iter().any(|(r, _, d)| r == rotulo && d);
    if destrutiva {
        // Volume é dado, não build. Confirmação SEMPRE, mesmo com --yes: um `--yes`
        // digitado pra limpar cache de build não pode levar o banco de dev junto.
        println!("\x1b[31mEsta poda APAGA DADOS\x1b[0m (volumes: banco de dev, uploads de teste).");
        if !confirma("tem certeza? digite 'sim' para confirmar: ") {
            println!("cancelado.");
            return Ok(());
        }
    } else if !yes && !confirma(&format!("podar \"{rotulo}\"? [s/N] ")) {
        println!("cancelado.");
        return Ok(());
    }
    let saida = docker::podar(&rotulo)?;
    println!("{}", saida.trim());
    Ok(())
}

/// Pergunta no terminal. Aceita `s`/`sim`/`y`/`yes`.
fn confirma(prompt: &str) -> bool {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut l = String::new();
    if std::io::stdin().read_line(&mut l).is_err() {
        return false;
    }
    matches!(l.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes")
}

/// Rótulo do comando na ajuda (mantém o i18n perto de quem usa).
pub(crate) fn _titulo() -> String {
    t("disco.title")
}
