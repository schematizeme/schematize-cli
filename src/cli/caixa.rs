//! `overflow overdev add` / `overflow overdev caixa` — a caixa de entrada do overdev.
//!
//! Serve pra jogar "isso também precisa" no projeto ENQUANTO um agente trabalha, sem
//! interromper ninguém e sem risco de perder a demanda. A lógica (e as garantias de
//! concorrência) mora em `overdev::caixa`; aqui é só a face de linha de comando.

use crate::cli::args::CaixaCmd;
use schematize::overdev::caixa;
use schematize::agentrun;
use std::path::PathBuf;

fn raiz() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))
}

/// Captura uma demanda. Retorna na hora — não toca no checklist.
pub(crate) fn caixa_add(texto: &str) -> Result<(), String> {
    let root = raiz()?;
    let id = caixa::adicionar(&root, texto)?;
    println!("demanda capturada ({id}).");
    println!("o checklist NÃO foi tocado — nenhum agente foi interrompido.");
    println!("organize com: overflow overdev caixa agente");
    Ok(())
}

pub(crate) fn caixa_cmd(sub: CaixaCmd) -> Result<(), String> {
    let root = raiz()?;
    match sub {
        CaixaCmd::List => {
            let p = caixa::pendentes(&root);
            let q = caixa::processadas(&root);
            if p.is_empty() && q.is_empty() {
                println!("caixa vazia.");
                return Ok(());
            }
            if !p.is_empty() {
                println!("\x1b[1mA ORGANIZAR\x1b[0m ({} demanda(s)) — texto cru, como você escreveu:", p.len());
                for e in &p {
                    println!("  {}\n    {}", e.id, elide(&e.texto, 100));
                }
            }
            if !q.is_empty() {
                println!("\n\x1b[1mA FUNDIR\x1b[0m ({} demanda(s)) — já viraram itens:", q.len());
                for e in &q {
                    println!("  {}", e.id);
                    for i in &e.itens {
                        println!("    - {i}");
                    }
                }
                println!("\nfunda com: overflow overdev caixa merge");
            }
            Ok(())
        }
        CaixaCmd::Organizar { id, itens } => {
            caixa::organizar(&root, &id, itens)?;
            println!("demanda {id} organizada. funda com: overflow overdev caixa merge");
            Ok(())
        }
        CaixaCmd::Merge => {
            let n = caixa::mesclar(&root)?;
            if n == 0 {
                println!("nada a fundir.");
            } else {
                println!("{n} item(ns) acrescentado(s) ao checklist.");
            }
            Ok(())
        }
        CaixaCmd::Agente => agente(&root),
    }
}

/// Abre um agente num TERMINAL pra transformar as demandas cruas em itens.
///
/// Terminal externo, e não embutido, pelo mesmo motivo do resto do app: o agente mexe
/// no projeto, e mexer sem a pessoa ver acontecer é o que a casa não faz. O prompt
/// manda ele devolver o resultado PELO CLI (`caixa organizar`), não editando o
/// checklist — assim ele nunca disputa o arquivo com o overdev que está rodando.
fn agente(root: &std::path::Path) -> Result<(), String> {
    let p = caixa::pendentes(root);
    if p.is_empty() {
        println!("nenhuma demanda a organizar.");
        return Ok(());
    }
    let bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "overflow".into());
    let prompt = format!(
        "Você vai organizar demandas novas de um projeto que JÁ TEM um overdev rodando.\n\
         \n\
         REGRA DURA: não edite `CHECKLIST.md` nem nenhum arquivo do overdev. Outro agente \
         pode estar escrevendo neles agora, e sua edição sobrescreveria o trabalho dele.\n\
         \n\
         Para cada demanda listada por `{bin} overdev caixa list`:\n\
         1. leia o texto cru;\n\
         2. quebre em itens de checklist pequenos, verificáveis e independentes;\n\
         3. registre com: {bin} overdev caixa organizar <id> --item \"...\" --item \"...\"\n\
         \n\
         Quando terminar todas, rode: {bin} overdev caixa merge\n\
         Esse comando é o ÚNICO que toca o checklist, e ele o faz sob trava.\n\
         \n\
         Demandas a organizar: {}",
        p.len()
    );
    println!("abrindo um agente no terminal pra organizar {} demanda(s)…", p.len());
    match agentrun::launch_prompt_in_terminal(root, &prompt) {
        Ok(msg) => {
            println!("{msg}");
            Ok(())
        }
        // Sem terminal gráfico (servidor, sessão SSH): em vez de falhar, entrega o
        // prompt pra a pessoa rodar onde quiser. A demanda já está capturada de
        // qualquer jeito — não se perde por não haver janela.
        Err(e) => {
            println!("não consegui abrir um terminal ({e}). rode o agente você mesmo:\n");
            println!("--- prompt ---\n{prompt}\n--------------");
            Ok(())
        }
    }
}

/// Corta um texto no comprimento dado (seguro a UTF-8).
fn elide(s: &str, n: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= n {
        return s;
    }
    s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
}
