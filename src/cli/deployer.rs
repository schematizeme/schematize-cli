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
    let faltam: Vec<_> = EXTERNOS.iter().filter(|a| !descobrir_app(a.bin).utilizavel()).collect();
    if !faltam.is_empty() {
        println!();
        println!("Para instalar um que falte:");
        for a in &faltam {
            // O comando do PRÓPRIO schematize, não o `curl`: "instalar pelo schematize" é o
            // que se prometeu, e mandar colar uma linha de curl é dar instrução, não instalar.
            println!("    schematize apps instalar {}", a.bin);
        }
        println!();
        println!("(ou, sem o schematize: {})", deployerlink::como_instalar_app(faltam[0].flag));
    }
    Ok(())
}

/// **O quê:** instala um app do ecossistema **de verdade** — roda o `install.sh` com a flag
/// dele, herdando o terminal.
///
/// **Onde:** `schematize apps instalar <app>`.
///
/// **Por que agora instala, se antes só imprimia o comando:** "instalar pelo schematize" era
/// a promessa, e imprimir uma linha para a pessoa colar não é instalar — é dar instrução. A
/// razão original (compilar leva minutos e pede rede) continua verdadeira, e por isso a
/// operação **avisa e confirma** em vez de simplesmente não existir.
///
/// **Herda o terminal de propósito:** a compilação leva minutos e o `install.sh` pede sudo
/// para as libs. Capturar a saída deixaria a pessoa olhando um cursor parado, e o pedido de
/// senha não teria onde aparecer.
pub(crate) fn apps_instalar(app: Option<String>, yes: bool) -> Result<(), String> {
    use schematize::deployerlink::{descobrir_app, externo, EXTERNOS};

    let Some(nome) = app else {
        // Sem nome: mostra o que falta, em vez de escolher por conta própria.
        let faltam: Vec<_> =
            EXTERNOS.iter().filter(|a| !descobrir_app(a.bin).utilizavel()).collect();
        if faltam.is_empty() {
            println!("todos os apps do ecossistema já estão instalados.");
            return Ok(());
        }
        println!("Apps que faltam:");
        for a in faltam {
            println!("  {:<12} {}", a.bin, a.sobre);
        }
        println!();
        println!("Instale com: schematize apps instalar <app>");
        return Ok(());
    };

    // Deny-by-default: só o que está na tabela. Um nome desconhecido não vira flag inventada
    // no instalador — isso passaria `--qualquercoisa` para um script que roda com sudo.
    let Some(a) = externo(&nome) else {
        let nomes: Vec<&str> = EXTERNOS.iter().map(|x| x.bin).collect();
        return Err(format!("não conheço o app `{nome}`. Os que existem: {}", nomes.join(", ")));
    };

    if let Estado::Instalado { versao, caminho } = descobrir_app(a.bin) {
        println!("{} {versao} já está instalado em {}", a.bin, caminho.display());
        println!("Para atualizar, rode o mesmo comando — ele recompila do fonte:");
        println!("    {}", deployerlink::como_instalar_app(a.flag));
        return Ok(());
    }

    let cmd = deployerlink::como_instalar_app(a.flag);
    println!("Vou instalar o `{}` — {}", a.bin, a.sobre);
    println!();
    println!("  Isto COMPILA do fonte e leva minutos. Precisa de rede, e o instalador");
    println!("  pode pedir sudo para as bibliotecas de build do sistema.");
    println!("  Comando: {cmd}");
    if !yes && !crate::cli::ssh::confirm("\n  Seguir? (s/N)") {
        println!("cancelado — nada foi feito.");
        return Ok(());
    }

    // Herda o terminal: a compilação é longa e o sudo precisa de onde perguntar.
    let st = std::process::Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .status()
        .map_err(|e| format!("não consegui iniciar a instalação: {e}"))?;
    if !st.success() {
        return Err(format!(
            "a instalação terminou com erro ({}). O schematize segue funcionando; \
             o `{}` é opcional.",
            st.code().map(|c| c.to_string()).unwrap_or_else(|| "sinal".into()),
            a.bin
        ));
    }
    match descobrir_app(a.bin) {
        Estado::Instalado { versao, .. } => println!("\n✓ {} {versao} instalado.", a.bin),
        // Veredito pelo ESTADO, não pelo código de saída do script: o install.sh é
        // best-effort com os apps opcionais, então ele pode sair 0 sem ter instalado.
        _ => println!(
            "\nO instalador terminou, mas o `{}` ainda não responde. Rode \
             `schematize apps` para ver o estado.",
            a.bin
        ),
    }
    Ok(())
}

/// **O quê:** repassa um comando a um app do ecossistema.
/// **Onde:** `schematize apps exec <app> -- <args>`.
pub(crate) fn apps_exec(app: String, args: Vec<String>) -> Result<(), String> {
    use schematize::deployerlink::{descobrir_app, externo, EXTERNOS};
    let Some(a) = externo(&app) else {
        let nomes: Vec<&str> = EXTERNOS.iter().map(|x| x.bin).collect();
        return Err(format!("não conheço o app `{app}`. Os que existem: {}", nomes.join(", ")));
    };
    match descobrir_app(a.bin) {
        Estado::Instalado { caminho, .. } => {
            let st = std::process::Command::new(&caminho)
                .args(&args)
                .status()
                .map_err(|e| format!("não consegui executar {}: {e}", caminho.display()))?;
            std::process::exit(st.code().unwrap_or(1));
        }
        _ => Err(format!(
            "`{}` não está instalado. Instale com:\n    schematize apps instalar {}",
            a.bin, a.bin
        )),
    }
}
