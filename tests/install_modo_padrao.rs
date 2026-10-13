//! O QUE: prova o MODO PADRÃO do `install.sh` (ADR-0022): binário pré-compilado, e
//! `--from-source` como opt-in. Flags antigas (`--binary`, `--source`) seguem aceitas.
//!
//! POR QUE EXISTE: até a v0.11.0 o padrão era compilar do fonte (commit 8893281). Em
//! 2026-10-13 o usuário inverteu: o leigo não deve precisar de rustup/compilador (§37.48).
//!
//! COMO: sourceia o `install.sh` real (`SCHEMATIZE_INSTALL_LIB=1`, sem instalar nada) com os
//! argumentos dados e imprime `$MODE`. Também confere que o fallback pro fonte existe e
//! avisa de forma acionável.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::Command;

/// **O quê:** `$MODE` resultante de sourcear o install.sh com `args`.
/// **Onde:** todos os testes deste arquivo.
fn modo(args: &str) -> String {
    let sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    let script = format!(". '{}' {args}; printf \"\\nMODO=%s\" \"$MODE\"", sh.display());
    let out = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SCHEMATIZE_INSTALL_LIB", "1")
        .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
        .output()
        .expect("bash roda");
    let so = String::from_utf8_lossy(&out.stdout).to_string();
    so.rsplit("MODO=").next().unwrap_or("").trim().to_string()
}

#[test]
fn padrao_sem_flags_e_binario() {
    assert_eq!(modo(""), "binary");
}

#[test]
fn from_source_e_opt_in() {
    assert_eq!(modo("--from-source"), "source");
    assert_eq!(modo("--source"), "source");
}

#[test]
fn flag_antiga_binary_continua_aceita() {
    assert_eq!(modo("--binary"), "binary");
}

#[test]
fn flags_de_app_nao_mudam_o_modo() {
    assert_eq!(modo("--deployer --git"), "binary");
}

#[test]
fn sem_binario_cai_pro_fonte_com_aviso_acionavel() {
    let sh = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
        .expect("le install.sh");
    assert!(sh.contains("binario_disponivel"), "falta a checagem de asset");
    assert!(sh.contains("vou compilar do fonte pra você"), "falta o aviso do fallback");
}
