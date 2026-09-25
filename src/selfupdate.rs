//! Self-update do CLI/GUI SEM sudo interativo — a causa do "cliquei em Atualizar e
//! nada acontece": o install.sh usa `sudo`, e o agente/GUI roda sem TTY, então o
//! sudo trava/falha calado. Aqui baixamos os binários pré-compilados do release e os
//! trocamos no lugar: dir do executável se for gravável; senão `pkexec` (prompt
//! gráfico no Linux); senão `~/.local/bin`. Tudo logado em update.log e com resultado
//! honesto (sucesso/erro), nunca engolido. Onde: chamado por agent (botão da
//! notificação), gui (Atualizar) e `schematize upgrade`.

// Só o caminho Unix consome estes itens; sem a guarda o build de Windows enche de
// aviso de código morto (o job de release não usa -D warnings, mas ruído esconde sinal).
use crate::util;
use std::fs;
use std::path::{Path, PathBuf};

/// ~/.claude/schematize/update.log — trilha de auditoria de cada tentativa.
fn log_path() -> PathBuf {
    util::config_path()
        .parent()
        .map(|p| p.join("update.log"))
        .unwrap_or_else(|| PathBuf::from("update.log"))
}

fn log(msg: &str) {
    let p = log_path();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    let line = format!("[{}] {}\n", util::now_unix(), msg);
    if let Ok(mut prev) = fs::read_to_string(&p).or_else(|_| Ok::<_, std::io::Error>(String::new()))
    {
        prev.push_str(&line);
        let _ = fs::write(&p, prev);
    }
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    // `-fsSL`: falha em HTTP >=400 (o 404 do asset ausente), SEM a barra de progresso (que vazava
    // como tabela `% Total % Received…` na mensagem de erro da GUI), mas ainda mostra o erro.
    util::run("curl", &["-fsSL", "-o", dest.to_str().ok_or("path inválido")?, url]).map(|_| ())
}

/// O binário baixado EXECUTA nesta máquina? (`<bin> --version` roda). Protege contra trocar por um
/// binário de glibc incompatível (ex.: build do Debian num openSUSE Leap) que brickaria a instalação.
///
/// **Por que NÃO é `#[cfg(unix)]`, embora já tenha sido.** A pergunta que ela faz — "o binário
/// que acabei de baixar roda aqui?" — vale em todo SO; o `cfg` era incidental, porque o único
/// chamador de então vivia sob `cfg(not(windows))`. Quando o [`ensure_gestor`] (ADR-0013) passou
/// a chamá-la de código cross-platform, a definição sumia no Windows e a chamada ficava órfã:
/// `cannot find function binary_runs in this scope`, que quebrou o job `windows` do release.
///
/// O que é específico de Unix é o `chmod +x` de antes da chamada — e esse continua com o `cfg`
/// dele, no chamador, que é onde ele pertence.
fn binary_runs(bin: &Path) -> bool {
    util::run(bin.to_str().unwrap_or_default(), &["--version"]).is_ok()
}

// ---------------------------------------------------------------------------
// DELEGAÇÃO AO GESTOR — `schematize-market` (ADR-0013).
//
// O app "depende" dele: toda atualização passa pelo gestor (que é instalado se faltar),
// cobrindo instalação limpa E update. Se ele não puder ser instalado ou disparado, cai no
// fluxo interno (binário/fonte) como rede de segurança.
//
// ATÉ O ADR-0013 o gestor era o `schematize-updater`. Ele foi ABSORVIDO pelo market, que
// passou a ser o único responsável por instalar e atualizar. Este módulo aponta para o
// sucessor, e não há caminho de volta ao antecessor de propósito: manter os dois seria
// manter dois modelos do que está instalado, para divergirem — que é o problema que a
// unificação existe para acabar. Uma máquina que só tenha o updater antigo continua
// atendida: o `ensure_gestor` baixa o market pré-compilado, e se nem isso der, o fluxo
// interno abaixo ainda atualiza o app.
// ---------------------------------------------------------------------------

/// Nome do binário do gestor nesta plataforma.
fn gestor_filename() -> &'static str {
    if cfg!(windows) {
        "schematize-market.exe"
    } else {
        "schematize-market"
    }
}

/// Asset do gestor pra esta plataforma. `None` se não há binário publicado.
///
/// Bate com o `release.yml` do `schematize_market_rs` e com o
/// `nucleo::plataforma::market_asset_name()` de lá — os dois são travados por teste no repo
/// do market, que é onde a lista nasce.
fn gestor_asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("schematize-market-linux-x86_64"),
        ("macos", "aarch64") => Some("schematize-market-macos-arm64"),
        ("macos", "x86_64") => Some("schematize-market-macos-x86_64"),
        ("windows", "x86_64") => Some("schematize-market-windows-x86_64.exe"),
        _ => None,
    }
}

/// Resolve o gestor no `$PATH` + `~/.cargo/bin` + `~/.local/bin`. `None` se ausente.
/// Exposto pra GUI checar na abertura ("pede pra instalar se faltar").
pub fn gestor_bin() -> Option<PathBuf> {
    let name = gestor_filename();
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    for sub in [".cargo/bin", ".local/bin"] {
        let p = util::home().join(sub).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Garante o gestor instalado: devolve o caminho; se faltar, baixa o asset da plataforma pro
/// `~/.cargo/bin`. Exposto pra GUI (botão "instalar gestor de atualizações").
pub fn ensure_gestor() -> Result<PathBuf, String> {
    if let Some(p) = gestor_bin() {
        return Ok(p);
    }
    let asset = gestor_asset().ok_or("sem binário do gestor pra esta plataforma/arch")?;
    let url = format!(
        "https://github.com/schematizeme/schematize_market_rs/releases/latest/download/{asset}"
    );
    let dst = util::home().join(".cargo").join("bin").join(gestor_filename());
    if let Some(parent) = dst.parent() {
        let _ = fs::create_dir_all(parent);
    }
    download(&url, &dst)?;
    #[cfg(unix)]
    {
        let _ = util::run("chmod", &["+x", dst.to_str().unwrap_or_default()]);
    }
    // O binário TEM de executar aqui. Um asset da glibc/arquitetura errada baixa com HTTP
    // 200 e não roda; deixá-lo no PATH criaria um "gestor" que responde a nada, e o sintoma
    // seria "cliquei em atualizar e não aconteceu nada" — de novo.
    if !binary_runs(&dst) {
        let _ = fs::remove_file(&dst);
        return Err("o gestor baixado não executa nesta máquina (glibc/arquitetura?)".into());
    }
    log(&format!("schematize-market instalado em {}", dst.display()));
    Ok(dst)
}

/// **O quê:** o gestor APOSENTADO (`schematize-updater`) ainda está na máquina? Devolve o
/// caminho, se estiver.
///
/// **Onde:** o `schematize doctor`, para apontá-lo sem apagá-lo.
///
/// **Por que só apontar:** apagar binário da máquina de alguém é ação do gestor, que o faz
/// dizendo o que fez (ADR-0013). Um diagnóstico que remove software em silêncio é o oposto
/// do que a palavra "doctor" promete.
pub fn updater_aposentado_presente() -> Option<PathBuf> {
    let name = if cfg!(windows) { "schematize-updater.exe" } else { "schematize-updater" };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    for sub in [".cargo/bin", ".local/bin"] {
        let p = util::home().join(sub).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Dispara `schematize-market update`. Unix: num TERMINAL externo (pode compilar → precisa
/// TTY). Windows: spawna (abre console). `Err` se não conseguir — o chamador cai no fluxo
/// interno.
fn run_gestor(up: &Path) -> Result<String, String> {
    #[cfg(unix)]
    {
        let script = format!(
            "#!/usr/bin/env bash\nexport PATH=\"$HOME/.cargo/bin:$HOME/.local/bin:$PATH\"\n{up:?} update\necho\nread -rp '[update encerrado — Enter para fechar] '\n"
        );
        let tmp = log_path().parent().unwrap_or(Path::new(".")).join("run-gestor.sh");
        fs::write(&tmp, &script).map_err(|e| e.to_string())?;
        let _ = util::run("chmod", &["+x", tmp.to_str().unwrap_or_default()]);
        let terms = [
            "konsole",
            "gnome-terminal",
            "xfce4-terminal",
            "x-terminal-emulator",
            "alacritty",
            "kitty",
            "xterm",
        ];
        let term = terms
            .iter()
            .find(|t| which(t))
            .ok_or_else(|| "nenhum terminal encontrado".to_string())?;
        let mut cmd = std::process::Command::new(term);
        match *term {
            "gnome-terminal" | "xfce4-terminal" => {
                cmd.arg("--").arg("bash").arg(&tmp);
            }
            _ => {
                cmd.arg("-e").arg("bash").arg(&tmp);
            }
        }
        cmd.spawn().map_err(|e| format!("abrir terminal: {e}"))?;
        Ok("Abri o schematize-market num terminal — ele atualiza o app (build incremental).".into())
    }
    #[cfg(windows)]
    {
        std::process::Command::new(up)
            .arg("update")
            .spawn()
            .map_err(|e| format!("iniciar o gestor: {e}"))?;
        Ok("schematize-market rodando — ele atualiza o app.".into())
    }
}

/// **O quê:** atualiza o app — DELEGANDO ao `schematize-market`, que é o dono disso.
///
/// **Onde:** `schematize upgrade`, o botão da janela, e o agente residente.
///
/// ## O fluxo interno FOI APAGADO, e essa é a fase E4 do ADR-0018
///
/// Até aqui esta função tentava o gestor e, se ele falhasse, **fazia a atualização sozinha**:
/// resolvia a tag mais nova, baixava os dois binários, conferia se executavam, trocava por
/// `rename`, e caía para compilar do fonte quando não havia asset. Cento e vinte linhas.
///
/// O `schematize-market` faz **exatamente isso**, e melhor: ele tem as três camadas da troca do
/// próprio binário (`rename(2)` atômico, verificação da cópia já gravada, e a retentativa de
/// `ETXTBSY`) que esta cópia não tinha. O ADR-0013 decidiu que ele é o dono de instalar e
/// atualizar; o que ficou aqui foi o antecessor que ninguém apagou.
///
/// **Duas lógicas de trocar o próprio binário é o pior lugar possível para ter duas lógicas.**
/// O modo de falha delas é deixar a máquina sem gestor nenhum, e a que diverge é a que roda
/// menos — ou seja, a que ninguém exercita.
///
/// ## Sem o gestor, isto FALHA e diz como resolver — não faz um trabalho pior em silêncio
///
/// O `ensure_gestor` instala o market quando ele falta, e só desiste quando nem isso é possível
/// (sem rede, plataforma fora da matriz, binário que não executa aqui). Aí o certo é o piso 4:
/// erro visível, com o comando que conserta. Cair num caminho degradado que ninguém testa é o
/// que produz o "cliquei em atualizar e não aconteceu nada" — o sintoma que este ecossistema
/// já perseguiu três vezes.
pub fn run() -> Result<String, String> {
    let cur = env!("CARGO_PKG_VERSION");
    log(&format!("self-update: delegando ao gestor (atual v{cur}) em {}", std::env::consts::OS));

    let gestor = ensure_gestor().map_err(|e| {
        format!(
            "{e}\n\nQuem atualiza este app é o `schematize-market` (ADR-0013), e ele não está \
             disponível. Instale-o e repita:\n  curl -fsSL {} | bash\n\nOu, se preferir \
             compilar do fonte agora:\n  curl -fsSL {} | bash -s -- --from-source",
            crate::upgrade::INSTALL_SH,
            crate::upgrade::INSTALL_SH
        )
    })?;
    run_gestor(&gestor)
}

#[cfg(unix)]
fn which(cmd: &str) -> bool {
    util::run("sh", &["-c", &format!("command -v {cmd}")]).is_ok()
}

#[cfg(test)]
mod tests_delegacao {
    /// **O hub NÃO baixa nem compila nada para se atualizar — ele delega.**
    ///
    /// Este teste lê o próprio módulo e reprova a volta do fluxo interno. É a fase E4 do
    /// ADR-0018 virando verificação, e ele existe porque a cópia já esteve aqui: 120 linhas que
    /// resolviam a tag, baixavam os binários, conferiam execução e trocavam por `rename` — tudo
    /// duplicando o `schematize-market`, que o ADR-0013 tornou o dono disso em setembro.
    ///
    /// **Duas lógicas de trocar o próprio binário é o pior lugar possível para ter duas
    /// lógicas:** o modo de falha delas é deixar a máquina sem gestor nenhum, e a que diverge é
    /// a que roda menos — ou seja, a que ninguém exercita.
    ///
    /// O que CONTINUA aqui é a PONTE: achar o gestor, instalá-lo se faltar, e dispará-lo. Isso
    /// é o mecanismo da delegação, não a cópia dela — a mesma distinção do ADR-0018 sobre os
    /// `*link.rs`.
    #[test]
    fn o_hub_delega_a_atualizacao_e_nao_a_refaz() {
        let fonte = include_str!("selfupdate.rs");
        let producao = fonte.split("#[cfg(test)]").next().expect("há código antes dos testes");

        // A ponte tem de ser CHAMADA pelo `run`, e não só existir no arquivo.
        //
        // A primeira versão deste teste checava presença no módulo, e passou quando eu troquei
        // o `ensure_gestor()` por um `gestor_bin()` dentro do `run` — o arquivo continha as
        // duas palavras, e o teste aprovou uma delegação que havia deixado de instalar o gestor
        // quando ele falta. **Presença não é chamada**, e é a mesma classe do teste de
        // `--version` que exigia ORDEM e não só existência.
        let i = producao.find("pub fn run()").expect("o run existe");
        let corpo_do_run = &producao[i..];
        for ponte in ["ensure_gestor", "run_gestor"] {
            assert!(
                corpo_do_run.contains(ponte),
                "o `run` não chama `{ponte}` — sem isso não há delegação, e o hub volta a \
                 depender de o gestor já estar na máquina"
            );
        }
        assert!(producao.contains("pub fn gestor_bin"), "a GUI usa o `gestor_bin` para o badge");

        // E o fluxo interno NÃO pode voltar. Cada nome aqui é uma peça da cópia que saiu.
        for morto in [
            "asset_names",
            "bin_filename",
            "upgrade_from_source_in_terminal",
            "releases/download",
            "latest_release_tag",
        ] {
            assert!(
                !producao.contains(morto),
                "`{morto}` voltou ao hub: quem baixa e troca binário é o `schematize-market` \\
                 (ADR-0013). Uma segunda implementação aqui é a que diverge, porque é a que \\
                 quase nunca roda."
            );
        }
    }

    /// **Sem o gestor, a falha é VISÍVEL e diz o comando** — não um caminho degradado silencioso.
    ///
    /// É o piso 4 e o §37.48: a mensagem ensina o que fazer, em vez de o software fazer um
    /// trabalho pior sem avisar. O sintoma que isso evita — "cliquei em atualizar e não
    /// aconteceu nada" — já apareceu três vezes neste ecossistema.
    #[test]
    fn sem_gestor_o_erro_ensina_o_caminho() {
        let fonte = include_str!("selfupdate.rs");
        let producao = fonte.split("#[cfg(test)]").next().unwrap();
        let i = producao.find("pub fn run()").expect("o run existe");
        let corpo = &producao[i..];
        assert!(corpo.contains("map_err"), "o erro do `ensure_gestor` tem de ser enriquecido");
        assert!(
            corpo.contains("INSTALL_SH"),
            "a mensagem tem de trazer o comando que instala o gestor — erro sem saída é erro \\
             que manda a pessoa adivinhar"
        );
    }
}
