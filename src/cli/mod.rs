//! Submódulos do BINÁRIO `schematize` — um por área de subcomando.
//!
//! O `main.rs` guarda só o `fn main` e o `match` de despacho; o trabalho de
//! cada subcomando mora aqui (piso da casa: <=750 linhas, uma unidade lógica
//! por arquivo). Fica em `src/cli/` e não solto em `src/` porque `src/*.rs` é o
//! espaço de módulos do LIB — misturar os dois confunde quem lê.

pub(crate) mod args;
pub(crate) mod skills;
pub(crate) mod skillsproj;
pub(crate) mod overdev;
pub(crate) mod ssh;
pub(crate) mod vps;
pub(crate) mod mcp;
pub(crate) mod conta;
pub(crate) mod caixa;
pub(crate) mod db;
pub(crate) mod disco;
pub(crate) mod git;
pub(crate) mod diversos;
