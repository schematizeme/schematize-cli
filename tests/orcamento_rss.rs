//! Orçamento de RSS (BUDGET.md): sobe o binário de verdade e falha se o pico passar do teto.
//!
//! **O quê:** roda `schematize <cmd>` com HOME descartável, polla `VmHWM` de
//! `/proc/<pid>/status` até o processo sair e compara com o teto declarado em BUDGET.md.
//! O `doctor` roda com um `schematize-gui` FALSO de 96 MiB no HOME: se alguém voltar a ler o
//! binário inteiro em memória (como era antes), o pico passa de 96 MiB e o teste falha.
//!
//! **Onde:** gate local (`cargo test`). Sem `/proc` (não-Linux) pula com aviso explícito.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const MIB: u64 = 1024 * 1024;

/// **O quê:** lê o `VmHWM` (pico de RSS, em bytes) de um pid; `None` se o processo já saiu.
/// **Onde:** laço de `pico_rss`.
fn vm_hwm(pid: u32) -> Option<u64> {
    let s = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kb: u64 =
        s.lines().find(|l| l.starts_with("VmHWM:"))?.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// **O quê:** executa o binário com HOME dado e devolve o maior `VmHWM` observado (bytes).
/// **Onde:** cada teste deste arquivo.
fn pico_rss(home: &Path, args: &[&str]) -> u64 {
    let mut filho = Command::new(env!("CARGO_BIN_EXE_schematize"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("sobe o schematize");
    let pid = filho.id();
    let mut pico = 0u64;
    loop {
        if let Some(v) = vm_hwm(pid) {
            pico = pico.max(v);
        }
        match filho.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(e) => panic!("try_wait falhou: {e}"),
        }
    }
    pico
}

/// **O quê:** HOME descartável sob o alvo do cargo, opcionalmente com GUI falsa de `gui_mib`.
/// **Onde:** início de cada teste.
fn home(nome: &str, gui_mib: u64) -> PathBuf {
    let h = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("teste-orcamento-rss")
        .join(format!("{}-{nome}", std::process::id()));
    let _ = fs::remove_dir_all(&h);
    fs::create_dir_all(h.join(".cargo/bin")).expect("cria HOME");
    if gui_mib > 0 {
        let mut f = File::create(h.join(".cargo/bin/schematize-gui")).expect("cria GUI falsa");
        f.write_all(b"slint").expect("escreve marcador");
        f.set_len(gui_mib * MIB).expect("estende GUI falsa");
    }
    h
}

/// **O quê:** confere `pico <= teto_mib` ou pula com aviso se `/proc` não existe.
/// **Onde:** fim de cada teste.
fn confere(op: &str, pico: u64, teto_mib: u64) {
    if !Path::new("/proc/self/status").exists() {
        eprintln!("PULADO ({op}): /proc indisponível nesta plataforma");
        return;
    }
    assert!(pico > 0, "{op}: não consegui ler VmHWM (processo terminou rápido demais?)");
    assert!(
        pico <= teto_mib * MIB,
        "{op}: pico {:.1} MiB > teto {teto_mib} MiB (BUDGET.md)",
        pico as f64 / MIB as f64
    );
}

/// `doctor` com GUI de 96 MiB: o pico não pode crescer com o tamanho do arquivo.
#[test]
fn doctor_nao_carrega_gui_inteira_em_memoria() {
    let h = home("doctor", 96);
    confere("doctor", pico_rss(&h, &["doctor"]), 16);
}

/// `status`: operação leve, teto de 16 MiB (medido ~9,4 MiB em debug).
#[test]
fn status_dentro_do_orcamento() {
    let h = home("status", 0);
    confere("status", pico_rss(&h, &["status"]), 16);
}

/// `--version`: arranque puro; o teto cobre o caso de a leitura pegar o processo já vivo.
#[test]
fn version_dentro_do_orcamento() {
    let h = home("version", 0);
    let pico = pico_rss(&h, &["--version"]);
    if pico == 0 {
        eprintln!(
            "--version saiu antes da 1a leitura de VmHWM: pico < 2 ms de vida, dentro do teto"
        );
        return;
    }
    confere("--version", pico, 16);
}
