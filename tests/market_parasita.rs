//! O parasita do market no BINÁRIO: roda nos comandos comuns e NUNCA nos hooks.
//!
//! **Onde:** contrato `market-parasita`. Sem rede: o `PATH` vazio faz o `curl` sumir, então o
//! parasita (quando roda) falha de forma observável — a linha de erro no stderr — em vez de baixar.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_schematize");

/// **O quê:** HOME vazio e único por teste.
///
/// **Onde:** os testes daqui.
fn home(nome: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("parasita-{}-{nome}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("criar o HOME de teste");
    d
}

/// **O quê:** roda o binário com HOME vazio, sem `curl` no PATH e SEM o opt-out.
///
/// **Onde:** os testes daqui.
fn rodar(home: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", "/nonexistent")
        .env_remove("CI")
        .env_remove("SCHEMATIZE_SEM_MARKET")
        .env_remove("SCHEMATIZE_DELEGADO_AO_MARKET")
        .output()
        .expect("rodar o binário")
}

/// Controle: um comando comum TENTA instalar (e, sem curl, diz por quê). Sem isto, o teste dos
/// hooks passaria mesmo com o parasita quebrado.
#[test]
fn comando_comum_tenta_instalar_e_o_app_segue() {
    let h = home("comum");
    let o = rodar(&h, &["lang", "--list"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("não consegui instalar o schematize-market"), "stderr: {err}");
    assert!(err.contains("o app segue normal"), "stderr: {err}");
    assert!(o.status.success(), "o app tem de seguir: {err}");
}

/// Os HOOKS (`overdev guard|check`) e `--version` não tocam no market: sem mensagem, sem pasta, sem lock.
#[test]
fn hooks_e_version_nao_rodam_o_parasita() {
    for args in [vec!["overdev", "guard"], vec!["overdev", "check"], vec!["--version"]] {
        let h = home("hook");
        let o = rodar(&h, &args);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            !err.contains("não consegui instalar o schematize-market"),
            "{args:?} rodou o parasita: {err}"
        );
        assert!(!h.join(".cargo").exists(), "{args:?} criou ~/.cargo");
        assert!(!h.join(".schematize").exists(), "{args:?} criou ~/.schematize");
    }
}

/// O opt-out vale no binário também.
#[test]
fn opt_out_no_binario() {
    let h = home("optout");
    let o = Command::new(BIN)
        .args(["lang", "--list"])
        .current_dir(&h)
        .env("HOME", &h)
        .env("PATH", "/nonexistent")
        .env("SCHEMATIZE_SEM_MARKET", "1")
        .output()
        .expect("rodar o binário");
    assert!(
        !String::from_utf8_lossy(&o.stderr).contains("não consegui instalar o schematize-market")
    );
    assert!(!h.join(".cargo").exists());
}
