//! `schematize upgrade` — RECOMPILA o CLI/GUI do fonte (source-first).
//! O quê: puxa o install.sh do main e roda em modo --from-source (rustup + libs de
//! build + `cargo install --features gui`), com stdio herdado pra sudo/rustup poderem
//! pedir no terminal. Não depende de binário publicado por CI.
//! Onde: chamado por main (`schematize upgrade`). A GUI/agente têm o atalho binário
//! (selfupdate.rs) pra quando existir release pronto; este é o caminho de verdade.

use crate::i18n::{t, tf};

/// O `install.sh` do `main` — a fonte ÚNICA desta URL no crate.
///
/// **Onde:** o fallback de recompilação do fonte ([`crate::selfupdate`]), este módulo, e a
/// mensagem que diz como instalar o gestor quando ele falta (`cli::deployer`).
///
/// Era declarada duas vezes, privada, com o mesmo literal. Duas cópias de uma URL é uma URL
/// que muda em um lugar só no dia em que mudar.
pub const INSTALL_SH: &str =
    "https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh";

/// Repo GitHub do PRÓPRIO app (a CLI/GUI) — usado pra resolver a versão mais nova do app.
const APP_REPO: &str = "schematize-cli";

/// Versão do app compilada (do Cargo.toml). Fonte de verdade do "estou na vX".
/// Onde: exibida no status/GUI e comparada com a última publicada em `app_update_available`.
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Há versão mais nova do PRÓPRIO app publicada? Retorna `Some((atual, latest))` se sim, `None`
/// se já está em dia OU se a checagem falhou (rede/GitHub) — resiliente, nunca panica.
/// O quê: reusa o MESMO mecanismo raw de `versoes::latest_version_raw` (lê a versão do `Cargo.toml`
/// no `main` via raw.githubusercontent, SEM a API 60/h) e compara semver com `util::semver_lt`.
/// Onde: consumido pela GUI (badge "Atualizar app") e pelo módulo de notificações.
pub fn app_update_available() -> Option<(String, String)> {
    let cur = app_version().to_string();
    let latest = crate::versoes::latest_version_raw(APP_REPO)?;
    if crate::util::semver_lt(&cur, &latest) {
        Some((cur, latest))
    } else {
        None
    }
}

/// **O quê:** `schematize upgrade` — DELEGA ao `schematize-market`, que é o dono de atualizar.
///
/// **Onde:** `main`, no despacho do subcomando.
///
/// ## A recompilação do fonte SAIU daqui (E4 do ADR-0018)
///
/// Esta função rodava `curl … install.sh | bash -s -- --from-source`, que é **exatamente** o que
/// o `schematize-market` faz no caminho de fonte dele. Duas lógicas para o mesmo ato, e a daqui
/// não tinha nenhuma das camadas que o market tem na troca do próprio binário.
///
/// O ADR-0013 deu esse dono ao market em setembro. O que ficou aqui foi o antecessor.
///
/// **`force` continua na assinatura e continua ignorado**, como já era: quem digitou
/// `schematize upgrade --force` não pode receber "flag desconhecida" porque a implementação
/// mudou de dono. Remover a flag é mudança de superfície, e tem guard próprio para isso.
pub fn run(_force: bool) -> Result<(), String> {
    println!("{}", t("upgrade.checking"));
    println!("{}", tf("upgrade.current", &[("v", app_version())]));
    // O `selfupdate::run` já resolve o gestor (instalando-o se faltar) e falha com a mensagem
    // que ensina o caminho quando nem isso dá. Um segundo tratamento aqui divergiria dele.
    let msg = crate::selfupdate::run()?;
    println!("{msg}");
    Ok(())
}
