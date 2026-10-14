//! `schematize apps install <app>` não pode mandar o market instalar o que ele não gere.
//!
//! **O quê:** roda o binário de verdade, com HOME descartável, e confere o que ele diz e o
//! código de saída para um app que o market não gere / de repo privado (`schematize-skills`).
//!
//! **Onde:** guarda do item B do release-gate (hub sem o motor do overdev).

use std::process::Command;

/// Roda `schematize apps install <app> --yes` num HOME vazio e devolve (rc, stdout+stderr).
fn instalar(app: &str) -> (i32, String) {
    let home =
        std::env::temp_dir().join(format!("schematize-appsinst-{}-{app}", std::process::id()));
    std::fs::create_dir_all(&home).expect("cria HOME descartável");
    let out = Command::new(env!("CARGO_BIN_EXE_schematize"))
        .args(["apps", "install", app, "--yes"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("PATH", "/usr/bin:/bin")
        .env("SCHEMATIZE_SEM_MARKET", "1")
        .output()
        .expect("executa o binário");
    let _ = std::fs::remove_dir_all(&home);
    let texto =
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), texto)
}

/// Skills: repo privado — não executa o market, sai != 0 e imprime o `cargo install --git`
/// com a nota do repo privado.
#[test]
fn app_nao_gerido_imprime_a_dica_e_sai_nao_zero() {
    let (rc, texto) = instalar("schematize-skills");
    assert_ne!(rc, 0, "{texto}");
    assert!(
        texto.contains(
            "cargo install --git https://github.com/schematizeme/schematize_skills_rs --locked"
        ),
        "{texto}"
    );
    assert!(texto.contains("ainda não é público"), "{texto}");
    assert!(!texto.contains("Vou instalar"), "não pode ter iniciado o fluxo do market: {texto}");
    assert!(!texto.contains("curl -fsSL"), "não é caso de instalar o gestor: {texto}");
}

/// **Nome curto no binário de verdade:** `apps install overdev` tem de cair no MESMO caminho de
/// `schematize-skills` (a dica do repo privado), e nunca em «não conheço o app». Cobre todos os
/// apps da tabela: o nome curto jamais pode ser tratado como desconhecido.
#[test]
fn nome_curto_nao_e_desconhecido_em_nenhum_app() {
    for app in [
        "deployer",
        "optimizer",
        "database",
        "git",
        "skills",
        "overdev",
        "market",
        "schematize-git",
    ] {
        let (_rc, texto) = instalar(app);
        assert!(
            !texto.contains("não conheço o app"),
            "`{app}` foi tratado como desconhecido: {texto}"
        );
    }
    let (rc, texto) = instalar("skills");
    assert_ne!(rc, 0, "{texto}");
    assert!(
        texto.contains("cargo install --git https://github.com/schematizeme/schematize_skills_rs"),
        "{texto}"
    );
    // Overdev agora é do market publicado: sem o gestor, pede o gestor (não a dica de fonte).
    let (rc, texto) = instalar("overdev");
    assert_ne!(rc, 0, "{texto}");
    assert!(texto.contains("schematize-market"), "{texto}");
    assert!(!texto.contains("cargo install --git"), "{texto}");
    // E o nome que realmente não existe segue sendo recusado.
    let (rc, texto) = instalar("naoexiste");
    assert_ne!(rc, 0, "{texto}");
    assert!(texto.contains("não conheço o app `naoexiste`"), "{texto}");
}
