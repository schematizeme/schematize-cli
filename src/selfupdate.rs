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
#[cfg(unix)]
use crate::versoes::latest_release_tag;
use std::fs;
use std::path::{Path, PathBuf};

const ORG: &str = "schematizeme";
const REPO: &str = "schematize-cli";

/// install.sh do main — usado pelo fallback de recompilação do fonte (source-first) quando
/// não há binário pré-compilado compatível pra plataforma (ex.: openSUSE, glibc diferente).
/// A URL tem um dono só (`upgrade`); aqui é só o apelido local.
#[cfg(unix)]
use crate::upgrade::INSTALL_SH;

/// Nomes dos assets por plataforma (batem com o que o CI publica).
#[cfg(unix)]
fn asset_names() -> (&'static str, &'static str) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        ("schematize-linux-x86_64", "schematize-gui-linux-x86_64")
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        ("schematize-macos-arm64", "schematize-gui-macos-arm64")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        ("schematize-macos-x86_64", "schematize-gui-macos-x86_64")
    }
    #[cfg(target_os = "windows")]
    {
        ("schematize-windows-x86_64.exe", "schematize-gui-windows-x86_64.exe")
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        target_os = "macos",
        target_os = "windows"
    )))]
    {
        ("schematize-linux-x86_64", "schematize-gui-linux-x86_64")
    }
}

#[cfg(unix)]
fn bin_filename(gui: bool) -> &'static str {
    #[cfg(target_os = "windows")]
    {
        if gui {
            "schematize-gui.exe"
        } else {
            "schematize.exe"
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if gui {
            "schematize-gui"
        } else {
            "schematize"
        }
    }
}

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

/// Diretório onde o executável atual vive (alvo padrão da troca).
#[cfg(unix)]
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Testa se dá pra escrever no diretório (sem depender de metadados de permissão).
#[cfg(unix)]
fn writable(dir: &Path) -> bool {
    let probe = dir.join(".schematize-write-probe");
    match fs::write(&probe, b"x") {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    // `-fsSL`: falha em HTTP >=400 (o 404 do asset ausente), SEM a barra de progresso (que vazava
    // como tabela `% Total % Received…` na mensagem de erro da GUI), mas ainda mostra o erro.
    util::run("curl", &["-fsSL", "-o", dest.to_str().ok_or("path inválido")?, url]).map(|_| ())
}

/// Recompila o app do FONTE num TERMINAL EXTERNO (rustup/sudo podem pedir senha lá — a GUI/agente
/// não têm TTY). É o fallback confiável quando não há binário pré-compilado pra a plataforma
/// (openSUSE e cia.) ou quando o binário baixado não roda aqui (glibc incompatível). Evita brickar
/// a instalação trocando por um binário que não executa. Devolve mensagem honesta.
#[cfg(unix)]
fn upgrade_from_source_in_terminal() -> Result<String, String> {
    let script = format!(
        "#!/usr/bin/env bash\nexport PATH=\"$HOME/.cargo/bin:$HOME/.local/bin:$PATH\"\ncurl -fsSL {INSTALL_SH} | bash -s -- --from-source\necho\nread -rp '[atualização encerrada — Enter para fechar] '\n"
    );
    let tmp = log_path().parent().unwrap_or(Path::new(".")).join("update-source.sh");
    fs::write(&tmp, &script).map_err(|e| format!("gravar script de update: {e}"))?;
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
    let term = terms.iter().find(|t| which(t)).ok_or_else(|| {
        format!(
            "sem binário pré-compilado pra sua distro e nenhum terminal encontrado. \
             Rode no terminal: curl -fsSL {INSTALL_SH} | bash -s -- --from-source"
        )
    })?;
    let mut cmd = std::process::Command::new(term);
    match *term {
        "gnome-terminal" | "xfce4-terminal" => {
            cmd.arg("--").arg("bash").arg(&tmp);
        }
        _ => {
            cmd.arg("-e").arg("bash").arg(&tmp);
        }
    }
    cmd.spawn().map_err(|e| format!("abrir terminal `{term}`: {e}"))?;
    log(&format!("fallback: recompilação do fonte aberta em `{term}`"));
    Ok("Sem binário pronto pra sua distro — abri um terminal recompilando do fonte \
        (rustup/sudo podem pedir senha). Reabra o app quando terminar."
        .into())
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

/// Executa a atualização. DELEGA ao `schematize-market` (instalando-o se faltar); só cai no
/// fluxo interno (binário/fonte) se o gestor não puder ser instalado/disparado.
pub fn run() -> Result<String, String> {
    let cur = env!("CARGO_PKG_VERSION");
    log(&format!("self-update: iniciando (atual v{cur}) em {}", std::env::consts::OS));

    // 1) Caminho CENTRAL: delega ao gestor (instala se faltar).
    match ensure_gestor() {
        Ok(up) => match run_gestor(&up) {
            Ok(msg) => return Ok(msg),
            Err(e) => log(&format!("run_gestor falhou ({e}) — caindo no fluxo interno")),
        },
        Err(e) => log(&format!("gestor indisponível ({e}) — caindo no fluxo interno")),
    }

    // Windows: substituir o .exe em execução é frágil — abre o release pra baixar o instalador.
    #[cfg(target_os = "windows")]
    {
        let url = format!("https://github.com/{ORG}/{REPO}/releases/latest");
        util::open_url(&url);
        log("windows: abri a página de releases (download manual do instalador)");
        return Ok("Abri a página de releases pra você baixar a versão nova do Windows.".into());
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Detecção pelo FONTE (raw main), não pela API 60/h — o que quebrava o versionamento.
        let tag = crate::versoes::latest_version_raw(REPO)
            .or_else(|| latest_release_tag(REPO))
            .ok_or_else(|| {
                "não consegui resolver a versão mais recente (rede/GitHub?)".to_string()
            })?;
        if tag == cur {
            log("já está na versão mais recente");
            return Ok(format!("Já está atualizado (v{cur})."));
        }
        let (cli_asset, gui_asset) = asset_names();
        let base = format!("https://github.com/{ORG}/{REPO}/releases/download/v{tag}");

        // Baixa os 2 binários pra uma pasta temporária dentro do config dir.
        let tmp = log_path().parent().unwrap_or(Path::new(".")).join("update-tmp");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let cli_tmp = tmp.join(bin_filename(false));
        let gui_tmp = tmp.join(bin_filename(true));
        // Baixa o binário do CLI. Sem asset pré-compilado (404) → NÃO falha feio: recompila do
        // fonte num terminal (openSUSE e qualquer distro sem release binário pronto).
        if download(&format!("{base}/{cli_asset}"), &cli_tmp).is_err() {
            let _ = fs::remove_dir_all(&tmp);
            log(&format!("sem binário pré-compilado v{tag} (404) — fallback pro fonte"));
            return upgrade_from_source_in_terminal();
        }
        let _ = util::run("chmod", &["+x", cli_tmp.to_str().unwrap()]);
        // Binário baixado EXECUTA aqui? (glibc compatível). Se não, NÃO troca — recompila do fonte
        // (evita trocar por um binário que não roda e brickar a instalação).
        if !binary_runs(&cli_tmp) {
            let _ = fs::remove_dir_all(&tmp);
            log("binário baixado não executa (glibc incompatível?) — fallback pro fonte");
            return upgrade_from_source_in_terminal();
        }
        // A GUI é opcional: se o asset não existir, segue só com o CLI.
        let has_gui = download(&format!("{base}/{gui_asset}"), &gui_tmp).is_ok();
        if has_gui {
            let _ = util::run("chmod", &["+x", gui_tmp.to_str().unwrap()]);
        }

        let dir = exe_dir();
        let cli_dst = dir.join(bin_filename(false));
        let gui_dst = dir.join(bin_filename(true));

        // 1) Dir do executável é gravável → troca no lugar (rename atômico).
        if writable(&dir) {
            place(&cli_tmp, &cli_dst)?;
            if has_gui {
                let _ = place(&gui_tmp, &gui_dst);
            }
            let _ = fs::remove_dir_all(&tmp);
            log(&format!("trocado em {} → v{tag}", dir.display()));
            return Ok(format!("Atualizado para v{tag}."));
        }

        // 2) Linux com pkexec → prompt gráfico de senha pra instalar no local do pacote.
        #[cfg(target_os = "linux")]
        if which("pkexec") {
            let mut sh = format!("install -m755 '{}' '{}'", cli_tmp.display(), cli_dst.display());
            if has_gui {
                sh.push_str(&format!(
                    " && install -m755 '{}' '{}'",
                    gui_tmp.display(),
                    gui_dst.display()
                ));
            }
            match util::run("pkexec", &["sh", "-c", &sh]) {
                Ok(_) => {
                    let _ = fs::remove_dir_all(&tmp);
                    log(&format!("trocado via pkexec em {} → v{tag}", dir.display()));
                    return Ok(format!("Atualizado para v{tag}."));
                }
                Err(e) => log(&format!("pkexec falhou: {e} — caindo pro ~/.local/bin")),
            }
        }

        // 3) Fallback sem privilégio: ~/.local/bin (precisa estar no PATH).
        let local = util::home().join(".local").join("bin");
        fs::create_dir_all(&local).map_err(|e| e.to_string())?;
        place(&cli_tmp, &local.join(bin_filename(false)))?;
        if has_gui {
            let _ = place(&gui_tmp, &local.join(bin_filename(true)));
        }
        let _ = fs::remove_dir_all(&tmp);
        log(&format!("instalado em {} → v{tag} (garanta ~/.local/bin no PATH)", local.display()));
        Ok(format!(
            "Atualizado para v{tag} em ~/.local/bin — garanta que ~/.local/bin está no seu PATH."
        ))
    }
}

/// Move o binário novo pro destino de forma atômica, mesmo com ele EM EXECUÇÃO.
///
/// O caso que isto cobre: `Text file busy` (ETXTBSY). No Linux não se abre pra escrita
/// um arquivo que está sendo executado — e o `schematize` está, porque o agente do
/// autostart roda o tempo todo. Um `fs::copy` direto no destino falha; um `rename(2)`
/// por cima, não: quem já roda continua no inode antigo e a próxima execução pega o
/// novo. Por isso o fallback também renomeia (grava ao lado, no MESMO diretório, pra
/// não cair em EXDEV) em vez de copiar por cima.
#[cfg(not(target_os = "windows"))]
fn place(src: &Path, dst: &Path) -> Result<(), String> {
    if fs::rename(src, dst).is_ok() {
        let _ = util::run("chmod", &["+x", dst.to_str().unwrap_or_default()]);
        return Ok(());
    }
    // FS diferente: grava ao lado do destino e renomeia por cima (nunca copia por cima).
    let tmp = dst.with_file_name(format!(
        "{}.novo",
        dst.file_name().and_then(|s| s.to_str()).unwrap_or("schematize")
    ));
    let _ = fs::remove_file(&tmp);
    fs::copy(src, &tmp).map_err(|e| format!("não consegui escrever {}: {e}", tmp.display()))?;
    let _ = util::run("chmod", &["+x", tmp.to_str().unwrap_or_default()]);
    fs::rename(&tmp, dst).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("não consegui substituir {}: {e}", dst.display())
    })
}

#[cfg(unix)]
fn which(cmd: &str) -> bool {
    util::run("sh", &["-c", &format!("command -v {cmd}")]).is_ok()
}
