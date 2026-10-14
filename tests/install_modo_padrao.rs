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

// ---------------------------------------------------------------------------
// KIT COMPLETO (`--completo`) e market SEMPRE na latest.
// ---------------------------------------------------------------------------

/// **O quê:** valor de uma variável do install.sh depois de sourceá-lo com `args`.
/// **Onde:** testes de parsing de `--completo`.
fn var(args: &str, nome: &str) -> String {
    let sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    let script = format!(". '{}' {args}; printf \"\\nVAL=%s\" \"${{{nome}:-?}}\"", sh.display());
    let out = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SCHEMATIZE_INSTALL_LIB", "1")
        .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
        .output()
        .expect("bash roda");
    let so = String::from_utf8_lossy(&out.stdout).to_string();
    so.rsplit("VAL=").next().unwrap_or("").trim().to_string()
}

#[test]
fn completo_e_sinonimos_ligam_o_kit() {
    assert_eq!(var("", "COMPLETO"), "0", "sem flag nao e kit completo");
    for f in ["--completo", "--all", "--kit"] {
        assert_eq!(var(f, "COMPLETO"), "1", "{f} deve ligar o kit completo");
    }
}

#[test]
fn completo_nao_muda_o_modo() {
    assert_eq!(modo("--completo"), "binary");
    assert_eq!(modo("--completo --from-source"), "source");
}

/// **O quê:** roda `install_market` com um `curl` falso e um `$HOME` descartável.
/// `market_antigo` é o conteúdo do binário que já está lá (None = não há).
/// `curl_ok` decide se o download "funciona". Devolve (saída combinada, conteúdo final).
/// **Onde:** testes de "market sempre latest".
fn roda_install_market(market_antigo: Option<&str>, curl_ok: bool, tag: &str) -> (String, String) {
    let raiz = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("market-{tag}"));
    let _ = std::fs::remove_dir_all(&raiz);
    let home = raiz.join("home");
    let stubs = raiz.join("stubs");
    let bin = home.join(".cargo/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&stubs).unwrap();
    let dst = bin.join("schematize-market");
    if let Some(c) = market_antigo {
        std::fs::write(&dst, c).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // curl falso: acha o `-o <arq>` e escreve nele um "market" que diz 9.9.9.
    let curl = if curl_ok {
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do [ \"$1\" = -o ] && { printf '#!/bin/sh\\necho \"schematize-market 9.9.9\"\\n' > \"$2\"; exit 0; }; shift; done\nexit 1\n"
    } else {
        "#!/bin/sh\nexit 22\n"
    };
    let c = stubs.join("curl");
    std::fs::write(&c, curl).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&c, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    let script = format!(". '{}'; install_market; echo RC=$?", sh.display());
    let path = format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap());
    let out = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SCHEMATIZE_INSTALL_LIB", "1")
        .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
        .env("HOME", &home)
        .env("PATH", path)
        .output()
        .expect("bash roda");
    let tudo =
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (tudo, std::fs::read_to_string(&dst).unwrap_or_default())
}

#[test]
fn market_novo_sobrescreve_market_antigo_mesmo_acima_do_piso() {
    if std::env::consts::ARCH != "x86_64" {
        return;
    }
    // Um market 0.9.0 saudavel, mas com self-update quebrado: o instalador NAO depende dele.
    let (saida, final_) = roda_install_market(
        Some("#!/bin/sh\necho 'schematize-market 0.9.0'\n"),
        true,
        "sobrescreve",
    );
    assert!(final_.contains("9.9.9"), "market antigo deveria ter sido trocado: {saida}");
    assert!(saida.contains("RC=0"), "{saida}");
}

#[test]
fn market_instala_quando_nao_existe() {
    if std::env::consts::ARCH != "x86_64" {
        return;
    }
    let (saida, final_) = roda_install_market(None, true, "novo");
    assert!(final_.contains("9.9.9"), "{saida}");
}

#[test]
fn download_falho_mantem_o_market_antigo_e_diz_o_que_fazer() {
    if std::env::consts::ARCH != "x86_64" {
        return;
    }
    let (saida, final_) =
        roda_install_market(Some("#!/bin/sh\necho 'schematize-market 0.9.0'\n"), false, "falha");
    assert!(final_.contains("0.9.0"), "nao pode apagar o market que funciona: {saida}");
    assert!(saida.contains("mantive o market"), "falta mensagem acionavel: {saida}");
    assert!(saida.contains("RC=0"), "{saida}");
}

#[test]
fn kit_completo_roda_os_tres_passos_e_reporta() {
    let sh = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
        .expect("le install.sh");
    for trecho in ["install --all -y", "schematize-skills", "doctor", "kit_completo"] {
        assert!(sh.contains(trecho), "install.sh sem {trecho}");
    }
}

/// **O quê:** roda `kit_completo` com binários falsos que gravam o que receberam num log.
/// `falha` é o nome do binário que deve sair com 1. Devolve (rc, saída, log de chamadas).
/// **Onde:** testes do kit completo.
fn roda_kit(falha: &str, tag: &str) -> (i32, String, String) {
    use std::os::unix::fs::PermissionsExt;
    let raiz = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("kit-{tag}"));
    let _ = std::fs::remove_dir_all(&raiz);
    let home = raiz.join("home");
    let bin = home.join(".cargo/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = raiz.join("chamadas.log");
    for nome in ["schematize", "schematize-market", "schematize-skills"] {
        let rc = if nome == falha { 1 } else { 0 };
        let corpo = format!("#!/bin/sh\necho \"{nome} $*\" >> '{}'\nexit {rc}\n", log.display());
        let f = bin.join(nome);
        std::fs::write(&f, corpo).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    let script = format!(". '{}'; kit_completo", sh.display());
    let out = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SCHEMATIZE_INSTALL_LIB", "1")
        .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
        .env("HOME", &home)
        .output()
        .expect("bash roda");
    let tudo =
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let chamadas = std::fs::read_to_string(&log).unwrap_or_default();
    (out.status.code().unwrap_or(-1), tudo, chamadas)
}

#[test]
fn kit_completo_tudo_ok_chama_os_tres_na_ordem_e_sai_zero() {
    let (rc, saida, chamadas) = roda_kit("nenhum", "ok");
    assert_eq!(rc, 0, "{saida}");
    let linhas: Vec<&str> = chamadas.lines().collect();
    assert_eq!(
        linhas,
        [
            "schematize-market install --all -y",
            "schematize-skills install --all",
            "schematize doctor"
        ],
        "{saida}"
    );
}

#[test]
fn kit_completo_passo_que_falha_nao_aborta_os_outros_e_sai_nao_zero() {
    let (rc, saida, chamadas) = roda_kit("schematize-market", "falha");
    assert_ne!(rc, 0, "{saida}");
    assert!(chamadas.contains("schematize-skills install --all"), "{saida}");
    assert!(chamadas.contains("schematize doctor"), "{saida}");
    assert!(saida.contains("FALHOU: market install --all -y"), "{saida}");
}
