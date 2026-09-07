//! Subcomandos da PONTE com o Deployer (`schematize deployer <sub>`).
//!
//! **O quê:** dizer se o Deployer está instalado, como instalá-lo, e repassar comandos a ele.
//!
//! **Onde:** despachado por `main.rs` (`Cmd::Deployer`). A descoberta mora em
//! [`schematize::deployerlink`]; aqui só há entrada e saída.
//!
//! ## Nada aqui pode falhar por o Deployer não existir
//!
//! `status` sem Deployer instalado é uma resposta **normal**, com o comando de instalação
//! ao lado — não um erro. É o piso 10 aplicado à própria ponte: a ausência de um app degrada
//! a experiência, nunca derruba o outro.

use crate::cli::args::*;
use schematize::deployerlink::{self, Estado};

/// Despacha `schematize deployer <sub>`.
pub(crate) fn deployer_cmd(sub: DeployerCmd) -> Result<(), String> {
    match sub {
        DeployerCmd::Status => {
            match deployerlink::descobrir() {
                Estado::Instalado { caminho, versao } => {
                    println!("deployer {versao} em {}", caminho.display());
                    println!("  SSH, VPS e acesso remoto auditado — `deployer --help`");
                }
                Estado::Ausente => {
                    println!("deployer: não instalado");
                    println!();
                    println!("Ele é o app de SSH/VPS, separado do schematize (ADR-0010).");
                    println!("Para instalar:");
                    println!("    {}", deployerlink::como_instalar());
                }
                Estado::Quebrado { caminho, erro } => {
                    // Terceiro estado explícito: dizer "ausente" para um binário que ESTÁ lá
                    // mandaria a pessoa reinstalar o que já tem e esconderia a causa.
                    println!("deployer: instalado em {} mas NÃO responde", caminho.display());
                    println!("  motivo: {erro}");
                    println!("  reinstale com: {}", deployerlink::como_instalar());
                }
            }
            Ok(())
        }

        DeployerCmd::Instalar => {
            if let Estado::Instalado { caminho, versao } = deployerlink::descobrir() {
                println!("deployer {versao} já está instalado em {}", caminho.display());
                println!("para atualizar, rode: {}", deployerlink::como_instalar());
                return Ok(());
            }
            // Não instala sozinho: compilar um app inteiro leva minutos e pede rede. Fazer
            // isso a partir de um subcomando curto seria surpresa cara — o comando fica
            // visível para a pessoa rodar quando quiser (e ver o que ele faz antes).
            println!("Para instalar o deployer, rode:");
            println!();
            println!("    {}", deployerlink::como_instalar());
            println!();
            println!("Ele compila do fonte junto com o schematize, no mesmo target");
            println!("compartilhado — as dependências pesadas não recompilam.");
            Ok(())
        }

        DeployerCmd::Exec { args } => match deployerlink::descobrir() {
            Estado::Instalado { caminho, .. } => {
                // Repasse puro: herda o terminal para que prompts (a passphrase do cofre!) e
                // saída interativa funcionem. E propaga o CÓDIGO DE SAÍDA — quem chama o
                // schematize em script decide por ele, e engolir o código transformaria uma
                // falha do deployer em sucesso aparente.
                let st = std::process::Command::new(&caminho)
                    .args(&args)
                    .status()
                    .map_err(|e| format!("não consegui executar {}: {e}", caminho.display()))?;
                std::process::exit(st.code().unwrap_or(1));
            }
            _ => Err(format!(
                "deployer não está instalado. Instale com:\n    {}",
                deployerlink::como_instalar()
            )),
        },
    }
}

/// **O quê:** `schematize apps` — o painel dos apps externos do ecossistema.
///
/// **Onde:** `main.rs` (`Cmd::Apps`).
///
/// **Por que existe, sendo que há `deployer status`:** com dois apps externos (e mais por
/// vir), perguntar um a um não escala, e quem não sabe que o `optimizer` existe nunca vai
/// digitar `schematize optimizer status`. O schematize é o **gestor de programas** da casa —
/// então ele mostra o catálogo.
pub(crate) fn apps_cmd() -> Result<(), String> {
    use schematize::deployerlink::{descobrir_app, EXTERNOS};
    // `{:<N}` conta BYTES, não caracteres: "não instalado" tem 13 caracteres e 14 bytes
    // (o `ã`), e o alinhamento quebra. Como a coluna é curta e fixa, o padding é calculado
    // por caractere — é o mesmo cuidado que qualquer saída em português exige.
    let pad = |t: &str, n: usize| -> String {
        let faltam = n.saturating_sub(t.chars().count());
        format!("{t}{}", " ".repeat(faltam))
    };
    println!("{} {} O QUE FAZ", pad("APP", 11), pad("ESTADO", 14));
    for a in EXTERNOS {
        let estado = match descobrir_app(a.bin) {
            Estado::Instalado { versao, .. } => format!("v{versao}"),
            Estado::Ausente => "não instalado".to_string(),
            Estado::Quebrado { .. } => "QUEBRADO".to_string(),
        };
        println!("{} {} {}", pad(a.bin, 11), pad(&estado, 14), a.sobre);
    }
    println!();
    println!("Para instalar um que falte:");
    for a in EXTERNOS {
        if !descobrir_app(a.bin).utilizavel() {
            println!("    {}", deployerlink::como_instalar_app(a.flag));
        }
    }
    Ok(())
}
