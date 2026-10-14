//! **market parasita** — todo binário do ecossistema garante, ao iniciar, que o gestor de
//! atualizações (`schematize-market` e `schematize-market-gui`) existe em `$HOME/.cargo/bin`.
//!
//! **O quê:** se faltar um dos dois, baixa o binário da plataforma (curl), valida (`--version`
//! sai 0) e só então move para o destino. Caminho rápido: dois `stat`, sem rede.
//!
//! **Onde:** a primeira coisa do `main` de cada binário (CLI e janela), depois do parse de
//! argumentos. Este arquivo é a MESMA cópia nos três repos (plataforma, sem domínio — D4 do
//! ADR-0016). Contrato: `.schematize/scratch/market-parasita/CONTRATO.md`.
//!
//! **Nunca derruba o app (piso 10):** toda falha vira uma linha no stderr e o app segue.
//! **Nunca é chamado pelos hooks** (`overdev guard|check`): eles rodam a cada chamada de
//! ferramenta e não podem ganhar rede nem latência.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// Os dois binários do gestor, na ordem em que são garantidos.
pub const NOMES: [&str; 2] = ["schematize-market", "schematize-market-gui"];

/// Idade a partir da qual um lock é considerado órfão (processo morto no meio do download).
pub const LOCK_VELHO: Duration = Duration::from_secs(10 * 60);

/// Comando de instalação manual citado na mensagem de falha.
const INSTALAR: &str =
    "curl -fsSL https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh | bash";

/// Desfecho de uma tentativa de garantir o gestor.
#[derive(Debug, PartialEq, Eq)]
pub enum Resultado {
    /// Os dois já existiam: nada feito, nenhuma rede.
    JaEstava,
    /// Outro processo cuida disso agora (lock recente).
    Ocupado,
    /// Instalou estes nomes.
    Instalou(Vec<String>),
    /// Não conseguiu: a causa, já legível.
    Falhou(String),
}

/// **O quê:** o nome do arquivo no disco (`.exe` no Windows).
///
/// **Onde:** [`o_que_falta`] e a montagem do destino.
fn arquivo(nome: &str, windows: bool) -> String {
    if windows {
        format!("{nome}.exe")
    } else {
        nome.to_string()
    }
}

/// **O quê:** quais dos dois binários NÃO existem em `pasta` (decisão pura: só `stat`).
///
/// **Onde:** [`garantir_em`], no caminho rápido.
pub fn o_que_falta(pasta: &Path) -> Vec<&'static str> {
    let win = cfg!(windows);
    NOMES.iter().copied().filter(|n| !pasta.join(arquivo(n, win)).is_file()).collect()
}

/// **O quê:** a URL do asset de `nome` para `os`/`arch` (como em `std::env::consts`), ou `None`
/// quando a plataforma não tem asset publicado. Alvos: linux/macos x86_64, macos arm64 e
/// windows x86_64.
///
/// **Onde:** [`garantir_em`].
pub fn url_do_asset(nome: &str, os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" | "arm64" => "arm64",
        _ => return None,
    };
    let sufixo = match (os, arch) {
        ("linux", "x86_64") | ("macos", "x86_64") | ("macos", "arm64") => "",
        ("windows", "x86_64") => ".exe",
        _ => return None,
    };
    Some(format!(
        "https://github.com/schematizeme/schematize_market_rs/releases/latest/download/{nome}-{os}-{arch}{sufixo}"
    ))
}

/// **O quê:** `true` se a linha de comando é um HOOK do Claude Code (`overdev guard|check`, ou
/// `guard|check|hooks` no `schematize-overdev`) — chamados a cada tool call/parada.
///
/// **Onde:** [`deve_rodar`].
pub fn eh_hook(args: &[String]) -> bool {
    let a = |i: usize| args.get(i).map(String::as_str);
    matches!((a(0), a(1)), (Some("overdev"), Some("guard" | "check")))
        || matches!(a(0), Some("guard" | "check" | "hooks"))
}

/// **O quê:** decide se o parasita roda, sobre o ambiente (`env`) e os `args` (sem o argv[0]).
/// Não roda em `--version`/`--help`, `desktop --install/--remover`, hooks, sob o market
/// (`SCHEMATIZE_DELEGADO_AO_MARKET=1`), nem com `SCHEMATIZE_SEM_MARKET=1` ou `CI` definida.
///
/// **Onde:** [`garantir_com`]. Pura: o ambiente entra por parâmetro.
pub fn deve_rodar(env: &dyn Fn(&str) -> Option<String>, args: &[String]) -> bool {
    if env("SCHEMATIZE_SEM_MARKET").as_deref() == Some("1")
        || env("SCHEMATIZE_DELEGADO_AO_MARKET").as_deref() == Some("1")
        || env("CI").is_some()
    {
        return false;
    }
    let tem = |f: &str| args.iter().any(|a| a == f);
    if tem("--version") || tem("-V") || tem("--help") || tem("-h") {
        return false;
    }
    if args.first().map(String::as_str) == Some("desktop") && (tem("--install") || tem("--remover"))
    {
        return false;
    }
    !eh_hook(args)
}

/// **O quê:** baixa `url` para `dst` com `curl -fsSL --max-time 30`.
///
/// **Onde:** o downloader real, injetado por [`garantir`]. Os testes injetam outro.
pub fn baixar_com_curl(url: &str, dst: &Path) -> Result<(), String> {
    let st = Command::new("curl")
        .args(["-fsSL", "--max-time", "30", "-o"])
        .arg(dst)
        .arg(url)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("curl indisponível: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("download falhou ({st}) em {url}"))
    }
}

/// **O quê:** marca `chmod +x` (unix) e exige que `<bin> --version` saia 0.
///
/// **Onde:** [`instalar_um`], antes do `rename`. Repete se o kernel responder `ETXTBSY` (26):
/// o arquivo acabou de ser escrito e outro `fork` pode ainda segurá-lo aberto.
fn validar(bin: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod: {e}"))?;
    }
    let mut ultimo = String::new();
    for _ in 0..5 {
        match Command::new(bin).arg("--version").stdin(Stdio::null()).output() {
            Ok(o) if o.status.success() => return Ok(()),
            Ok(o) => return Err(format!("`--version` saiu com {}", o.status)),
            Err(e) if e.raw_os_error() == Some(26) => {
                ultimo = e.to_string();
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("não executa: {e}")),
        }
    }
    Err(format!("não executa: {ultimo}"))
}

/// **O quê:** baixa, valida e move UM binário; qualquer falha remove o `.novo`.
///
/// **Onde:** [`garantir_em`], uma vez por binário que falta.
fn instalar_um(
    url: &str,
    dst: &Path,
    baixar: &dyn Fn(&str, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let mut novo = dst.as_os_str().to_owned();
    novo.push(".novo");
    let novo = PathBuf::from(novo);
    let r = baixar(url, &novo)
        .and_then(|_| validar(&novo))
        .and_then(|_| fs::rename(&novo, dst).map_err(|e| format!("rename: {e}")));
    match r {
        Err(causa) if novo.exists() => match fs::remove_file(&novo) {
            Ok(()) => Err(causa),
            Err(e) => Err(format!("{causa} (e não removi {}: {e})", novo.display())),
        },
        outro => outro,
    }
}

/// **O quê:** toma o lock com `create_new`. `Ok(true)` = é nosso; `Ok(false)` = outro processo
/// (lock com menos de [`LOCK_VELHO`]) cuida; lock mais velho é órfão: remove e tenta de novo.
///
/// **Onde:** [`garantir_em`].
fn tomar_lock(lock: &Path) -> Result<bool, String> {
    if let Some(dir) = lock.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("criar {}: {e}", dir.display()))?;
    }
    for _ in 0..2 {
        match fs::OpenOptions::new().write(true).create_new(true).open(lock) {
            Ok(mut f) => {
                f.write_all(std::process::id().to_string().as_bytes())
                    .map_err(|e| format!("escrever lock: {e}"))?;
                return Ok(true);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let idade = fs::metadata(lock)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .unwrap_or(LOCK_VELHO);
                if idade < LOCK_VELHO {
                    return Ok(false);
                }
                fs::remove_file(lock).map_err(|e| format!("remover lock órfão: {e}"))?;
            }
            Err(e) => return Err(format!("lock: {e}")),
        }
    }
    Ok(false)
}

/// **O quê:** o miolo testável: garante os binários em `pasta` com `lock`, para `os`/`arch`,
/// usando o downloader injetado `baixar`.
///
/// **Onde:** [`garantir_com`] e os testes. Efeito: pode criar `pasta`, escrever binários e
/// criar/remover `lock`.
pub fn garantir_em(
    pasta: &Path,
    lock: &Path,
    os: &str,
    arch: &str,
    baixar: &dyn Fn(&str, &Path) -> Result<(), String>,
) -> Resultado {
    let falta = o_que_falta(pasta);
    if falta.is_empty() {
        return Resultado::JaEstava;
    }
    match tomar_lock(lock) {
        Ok(true) => {}
        Ok(false) => return Resultado::Ocupado,
        Err(e) => return Resultado::Falhou(e),
    }
    let r = instalar_todos(&falta, pasta, os, arch, baixar);
    if let Err(e) = fs::remove_file(lock) {
        if r.is_ok() {
            return Resultado::Falhou(format!("não removi o lock: {e}"));
        }
    }
    match r {
        Ok(v) => Resultado::Instalou(v),
        Err(e) => Resultado::Falhou(e),
    }
}

/// **O quê:** instala cada nome de `falta`; para no primeiro erro.
///
/// **Onde:** [`garantir_em`], com o lock em mãos.
fn instalar_todos(
    falta: &[&str],
    pasta: &Path,
    os: &str,
    arch: &str,
    baixar: &dyn Fn(&str, &Path) -> Result<(), String>,
) -> Result<Vec<String>, String> {
    fs::create_dir_all(pasta).map_err(|e| format!("criar {}: {e}", pasta.display()))?;
    let mut feitos = Vec::new();
    for nome in falta {
        let url = url_do_asset(nome, os, arch)
            .ok_or_else(|| format!("sem binário publicado para {os}/{arch}"))?;
        let dst = pasta.join(arquivo(nome, os == "windows"));
        instalar_um(&url, &dst, baixar).map_err(|e| format!("{nome}: {e}"))?;
        feitos.push(nome.to_string());
    }
    Ok(feitos)
}

/// **O quê:** a linha do stderr para um [`Resultado`] (`None` = silêncio: nada a dizer).
///
/// **Onde:** [`garantir_com`].
pub fn mensagem(r: &Resultado, pasta: &Path) -> Option<String> {
    match r {
        Resultado::JaEstava | Resultado::Ocupado => None,
        Resultado::Instalou(v) => Some(format!(
            "schematize: instalei o gestor de atualizações ({}) em {}",
            v.join(", "),
            pasta.display()
        )),
        Resultado::Falhou(c) => Some(format!(
            "schematize: não consegui instalar o schematize-market ({c}) — o app segue normal; rode: {INSTALAR}"
        )),
    }
}

/// **O quê:** o pipeline completo sobre ambiente e downloader injetados: decide, resolve as
/// pastas a partir de `home`, instala e escreve a linha no stderr.
///
/// **Onde:** [`garantir`] (real) e os testes (opt-out e fluxo).
pub fn garantir_com(
    env: &dyn Fn(&str) -> Option<String>,
    args: &[String],
    home: Option<PathBuf>,
    baixar: &dyn Fn(&str, &Path) -> Result<(), String>,
) -> Option<Resultado> {
    if !deve_rodar(env, args) {
        return None;
    }
    let Some(home) = home else {
        eprintln!(
            "{}",
            mensagem(&Resultado::Falhou("HOME indefinido".into()), Path::new("?"))
                .unwrap_or_default()
        );
        return None;
    };
    let pasta = home.join(".cargo").join("bin");
    let lock = home.join(".schematize").join("run").join("market-bootstrap.lock");
    let r = garantir_em(&pasta, &lock, std::env::consts::OS, std::env::consts::ARCH, baixar);
    if let Some(m) = mensagem(&r, &pasta) {
        eprintln!("{m}");
    }
    Some(r)
}

/// **O quê:** garante o market na pasta canônica do usuário. Chame no início do `main`, depois
/// do parse de argumentos. Em `cfg(test)` não faz nada.
///
/// **Onde:** `main` de cada binário. Síncrono e limitado (30 s por download), só quando falta.
pub fn garantir() {
    #[cfg(not(test))]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let env = |k: &str| std::env::var(k).ok();
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        garantir_com(&env, &args, home, &baixar_com_curl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Pasta temporária única por teste (sem crate externo).
    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("market-parasita-{}-{nome}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn sem_env(_: &str) -> Option<String> {
        None
    }

    /// Downloader que grava um script que sai 0 (binário "válido").
    fn baixa_ok(_: &str, dst: &Path) -> Result<(), String> {
        fs::write(dst, "#!/bin/sh\nexit 0\n").map_err(|e| e.to_string())
    }

    #[test]
    fn falta_os_dois_um_ou_nenhum() {
        let d = tmp("falta");
        assert_eq!(o_que_falta(&d), NOMES.to_vec());
        fs::write(d.join(arquivo(NOMES[0], cfg!(windows))), "x").unwrap();
        assert_eq!(o_que_falta(&d), vec![NOMES[1]]);
        fs::write(d.join(arquivo(NOMES[1], cfg!(windows))), "x").unwrap();
        assert!(o_que_falta(&d).is_empty());
    }

    #[test]
    fn url_dos_quatro_alvos_e_um_sem_asset() {
        let base = "https://github.com/schematizeme/schematize_market_rs/releases/latest/download/";
        let u = |n, o, a| url_do_asset(n, o, a);
        assert_eq!(
            u("schematize-market", "linux", "x86_64"),
            Some(format!("{base}schematize-market-linux-x86_64"))
        );
        assert_eq!(
            u("schematize-market", "macos", "aarch64"),
            Some(format!("{base}schematize-market-macos-arm64"))
        );
        assert_eq!(
            u("schematize-market-gui", "macos", "x86_64"),
            Some(format!("{base}schematize-market-gui-macos-x86_64"))
        );
        assert_eq!(
            u("schematize-market-gui", "windows", "x86_64"),
            Some(format!("{base}schematize-market-gui-windows-x86_64.exe"))
        );
        assert_eq!(u("schematize-market", "linux", "aarch64"), None);
        assert_eq!(u("schematize-market", "freebsd", "x86_64"), None);
    }

    #[cfg(unix)]
    #[test]
    fn instala_e_nao_deixa_novo() {
        let d = tmp("ok");
        let lock = d.join("run/lock");
        let pasta = d.join("bin");
        let r = garantir_em(&pasta, &lock, "linux", "x86_64", &baixa_ok);
        assert_eq!(r, Resultado::Instalou(NOMES.iter().map(|s| s.to_string()).collect()));
        assert!(o_que_falta(&pasta).is_empty());
        let sobras: Vec<_> = fs::read_dir(&pasta)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".novo"))
            .collect();
        assert!(sobras.is_empty(), "sobrou {sobras:?}");
        assert!(!lock.exists(), "lock tem de ser removido");
        assert!(mensagem(&r, &pasta).unwrap().contains("instalei o gestor"));
    }

    #[cfg(unix)]
    #[test]
    fn binario_que_nao_executa_remove_o_novo_e_poupa_o_destino() {
        let d = tmp("ruim");
        let pasta = d.join("bin");
        fs::create_dir_all(&pasta).unwrap();
        // O primeiro já existe: tem de ficar intocado. Falta só o -gui.
        let existente = pasta.join(NOMES[0]);
        fs::write(&existente, "original").unwrap();
        let ruim = |_: &str, dst: &Path| {
            fs::write(dst, "isto nao e um binario").map_err(|e| e.to_string())
        };
        let r = garantir_em(&pasta, &d.join("lock"), "linux", "x86_64", &ruim);
        assert!(matches!(r, Resultado::Falhou(_)), "{r:?}");
        assert!(!pasta.join(format!("{}.novo", NOMES[1])).exists());
        assert!(!pasta.join(NOMES[1]).exists());
        assert_eq!(fs::read_to_string(&existente).unwrap(), "original");
        assert!(mensagem(&r, &pasta).unwrap().contains("o app segue normal"));
    }

    #[test]
    fn lock_recente_nao_faz_nada() {
        let d = tmp("lock");
        let lock = d.join("lock");
        fs::write(&lock, "outro").unwrap();
        let chamou = Cell::new(false);
        let espia = |_: &str, _: &Path| {
            chamou.set(true);
            Err("não devia".to_string())
        };
        let r = garantir_em(&d.join("bin"), &lock, "linux", "x86_64", &espia);
        assert_eq!(r, Resultado::Ocupado);
        assert!(!chamou.get());
        assert!(lock.exists(), "o lock do outro processo não é nosso para remover");
    }

    #[test]
    fn lock_orfao_e_removido_e_o_trabalho_segue() {
        let d = tmp("orfao");
        let lock = d.join("lock");
        fs::write(&lock, "morto").unwrap();
        let velho = SystemTime::now() - Duration::from_secs(11 * 60);
        fs::File::options().write(true).open(&lock).unwrap().set_modified(velho).unwrap();
        assert_eq!(tomar_lock(&lock), Ok(true));
    }

    #[test]
    fn sem_asset_nao_baixa_e_diz_o_motivo() {
        let d = tmp("semasset");
        let chamou = Cell::new(false);
        let espia = |_: &str, _: &Path| {
            chamou.set(true);
            Ok(())
        };
        let r = garantir_em(&d.join("bin"), &d.join("lock"), "linux", "aarch64", &espia);
        assert!(matches!(&r, Resultado::Falhou(c) if c.contains("sem binário publicado")), "{r:?}");
        assert!(!chamou.get());
        assert!(!d.join("lock").exists());
    }

    #[test]
    fn opt_out_nao_chama_o_downloader() {
        let d = tmp("optout");
        let chamou = Cell::new(false);
        let espia = |_: &str, _: &Path| {
            chamou.set(true);
            Ok(())
        };
        for var in ["SCHEMATIZE_SEM_MARKET", "SCHEMATIZE_DELEGADO_AO_MARKET"] {
            let env = move |k: &str| (k == var).then(|| "1".to_string());
            assert!(garantir_com(&env, &[], Some(d.clone()), &espia).is_none(), "{var}");
        }
        let ci = |k: &str| (k == "CI").then(|| "true".to_string());
        assert!(garantir_com(&ci, &[], Some(d.clone()), &espia).is_none());
        assert!(!chamou.get());
    }

    #[test]
    fn nao_roda_em_version_help_desktop_e_hooks() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for args in [
            v(&["--version"]),
            v(&["-V"]),
            v(&["--help"]),
            v(&["desktop", "--install"]),
            v(&["desktop", "--remover"]),
            v(&["overdev", "guard"]),
            v(&["overdev", "check"]),
            v(&["guard"]),
            v(&["check"]),
        ] {
            assert!(!deve_rodar(&sem_env, &args), "{args:?}");
        }
        for args in [v(&[]), v(&["status"]), v(&["overdev", "status"]), v(&["--dir", "/p"])] {
            assert!(deve_rodar(&sem_env, &args), "{args:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn fluxo_completo_por_garantir_com() {
        let d = tmp("fluxo");
        let r = garantir_com(&sem_env, &[], Some(d.clone()), &baixa_ok);
        assert!(matches!(r, Some(Resultado::Instalou(_))), "{r:?}");
        assert!(o_que_falta(&d.join(".cargo/bin")).is_empty());
        // Segunda vez: caminho rápido.
        assert_eq!(garantir_com(&sem_env, &[], Some(d), &baixa_ok), Some(Resultado::JaEstava));
    }
}
