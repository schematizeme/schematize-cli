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
