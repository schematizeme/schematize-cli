//! O QUE: prova de ponta a ponta (REDE) de que `ensure_gestor` troca um market abaixo do piso
//! pelo publicado. POR QUE `#[ignore]`: baixa do GitHub; roda a pedido, num HOME descartável
//! (`HOME=<scratch> cargo test --test piso_market_rede -- --ignored`).
//! DE ONDE: `$HOME/.cargo/bin/schematize-market` preexistente (um market velho).
//! PRA ONDE: o mesmo caminho, trocado de forma atômica; só asserção sobre `--version`.
#![cfg(target_os = "linux")]

/// Com um market velho no HOME, `ensure_gestor` o devolve substituído por versão >= piso.
#[test]
#[ignore = "baixa do GitHub; rodar com HOME descartavel"]
fn ensure_gestor_troca_market_abaixo_do_piso() {
    let alvo = schematize::util::home().join(".cargo/bin/schematize-market");
    assert!(alvo.is_file(), "pre-condicao: coloque um market velho em {}", alvo.display());
    let antes = schematize::util::run(alvo.to_str().unwrap(), &["--version"]).unwrap();
    let dst = schematize::selfupdate::ensure_gestor().expect("ensure_gestor");
    assert_eq!(dst, alvo);
    let depois = schematize::util::run(alvo.to_str().unwrap(), &["--version"]).unwrap();
    println!("antes:  {}\ndepois: {}", antes.trim(), depois.trim());
    assert_ne!(antes, depois, "o market nao foi trocado");
}
