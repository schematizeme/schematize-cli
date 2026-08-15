//! Self-update do CLI/GUI SEM sudo interativo — a causa do "cliquei em Atualizar e
//! nada acontece": o install.sh usa `sudo`, e o agente/GUI roda sem TTY, então o
//! sudo trava/falha calado. Aqui baixamos os binários pré-compilados do release e os
//! trocamos no lugar: dir do executável se for gravável; senão `pkexec` (prompt
//! gráfico no Linux); senão `~/.local/bin`. Tudo logado em update.log e com resultado
//! honesto (sucesso/erro), nunca engolido. Onde: chamado por agent (botão da
//! notificação), gui (Atualizar) e `schematize upgrade`.

use crate::skills::latest_release_tag;
use crate::util;
use std::fs;
use std::path::{Path, PathBuf};

const ORG: &str = "schematizeme";
const REPO: &str = "schematize-cli";

/// Nomes dos assets por plataforma (batem com o que o CI publica).
fn asset_names() -> (&'static str, &'static str) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { ("schematize-linux-x86_64", "schematize-gui-linux-x86_64") }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { ("schematize-macos-arm64", "schematize-gui-macos-arm64") }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { ("schematize-macos-x86_64", "schematize-gui-macos-x86_64") }
    #[cfg(target_os = "windows")]
    { ("schematize-windows-x86_64.exe", "schematize-gui-windows-x86_64.exe") }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        target_os = "macos",
        target_os = "windows"
    )))]
    { ("schematize-linux-x86_64", "schematize-gui-linux-x86_64") }
}

fn bin_filename(gui: bool) -> &'static str {
    #[cfg(target_os = "windows")]
    { if gui { "schematize-gui.exe" } else { "schematize.exe" } }
    #[cfg(not(target_os = "windows"))]
    { if gui { "schematize-gui" } else { "schematize" } }
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
    if let Ok(mut prev) = fs::read_to_string(&p).or_else(|_| Ok::<_, std::io::Error>(String::new())) {
        prev.push_str(&line);
        let _ = fs::write(&p, prev);
    }
}

/// Diretório onde o executável atual vive (alvo padrão da troca).
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Testa se dá pra escrever no diretório (sem depender de metadados de permissão).
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
    util::run("curl", &["-fSL", "-o", dest.to_str().ok_or("path inválido")?, url]).map(|_| ())
}

/// Executa a atualização. Retorna a mensagem de sucesso (versão) ou erro descritivo.
pub fn run() -> Result<String, String> {
    let cur = env!("CARGO_PKG_VERSION");
    log(&format!("self-update: iniciando (atual v{cur}) em {}", std::env::consts::OS));

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
        let tag = crate::skills::latest_version_raw(REPO)
            .or_else(|| latest_release_tag(REPO))
            .ok_or_else(|| "não consegui resolver a versão mais recente (rede/GitHub?)".to_string())?;
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
        download(&format!("{base}/{cli_asset}"), &cli_tmp)
            .map_err(|e| format!("falha ao baixar o CLI: {e}"))?;
        // A GUI é opcional: se o asset não existir, segue só com o CLI.
        let has_gui = download(&format!("{base}/{gui_asset}"), &gui_tmp).is_ok();
        let _ = util::run("chmod", &["+x", cli_tmp.to_str().unwrap()]);
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
            let mut sh = format!(
                "install -m755 '{}' '{}'",
                cli_tmp.display(),
                cli_dst.display()
            );
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

/// Move o binário novo pro destino de forma atômica (rename no mesmo FS; fallback cp).
#[cfg(not(target_os = "windows"))]
fn place(src: &Path, dst: &Path) -> Result<(), String> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    // FS diferente: copia e ajusta permissão.
    fs::copy(src, dst).map_err(|e| format!("não consegui escrever {}: {e}", dst.display()))?;
    let _ = util::run("chmod", &["+x", dst.to_str().unwrap()]);
    Ok(())
}

#[cfg(target_os = "linux")]
fn which(cmd: &str) -> bool {
    util::run("sh", &["-c", &format!("command -v {cmd}")]).is_ok()
}
