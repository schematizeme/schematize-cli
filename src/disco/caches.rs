//! Caches GLOBAIS das toolchains — o lixo que não mora em projeto nenhum.
//!
//! O quê: os diretórios que Rust, Go, Node e Python enchem no HOME, fora de qualquer
//! projeto. Onde: entram no inventário do `disco` junto com os artefatos.
//!
//! Por que separado dos artefatos: estes não são recriados por um build do projeto —
//! são baixados de novo da rede. Apagar custa banda e tempo, não trabalho. Por isso
//! aparecem numa categoria própria, pra a decisão ser consciente.

use super::{Achado, Tipo};
use crate::util;
use std::path::PathBuf;

/// Um cache global: caminho, tipo e como se refaz.
fn candidatos() -> Vec<(PathBuf, Tipo, &'static str)> {
    let home = util::home();
    let mut v = vec![
        (home.join(".cargo/registry/cache"), Tipo::CargoCache, "baixado de novo no próximo build"),
        (home.join(".cargo/registry/src"), Tipo::CargoCache, "reextraído do cache"),
        (home.join(".cargo/git/checkouts"), Tipo::CargoCache, "reclonado no próximo build"),
        (home.join(".npm/_cacache"), Tipo::NodeCache, "baixado de novo no npm i"),
        (home.join(".cache/go-build"), Tipo::GoCache, "recriado no próximo go build"),
        (home.join("go/pkg/mod"), Tipo::GoCache, "baixado de novo no go build"),
        (home.join(".cache/pip"), Tipo::PythonCache, "baixado de novo no pip install"),
        (home.join(".cache/uv"), Tipo::PythonCache, "baixado de novo no uv"),
    ];
    // O Go respeita GOCACHE/GOMODCACHE — se estiverem apontando pra outro lugar, é lá
    // que o espaço está sendo gasto, não no default.
    for (var, tipo, refaz) in [
        ("GOCACHE", Tipo::GoCache, "recriado no próximo go build"),
        ("GOMODCACHE", Tipo::GoCache, "baixado de novo no go build"),
    ] {
        if let Ok(saida) = util::run("go", &["env", var]) {
            let p = PathBuf::from(saida.trim());
            if p.is_absolute() && !v.iter().any(|(x, _, _)| *x == p) {
                v.push((p, tipo, refaz));
            }
        }
    }
    v
}

/// Os caches globais que existem nesta máquina e passam do corte de tamanho.
pub fn varrer(so_acima_de: u64) -> Vec<Achado> {
    let mut v: Vec<Achado> = candidatos()
        .into_iter()
        .filter_map(|(p, t, refaz)| super::medir(&p, t, refaz, so_acima_de))
        .collect();
    v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Todo candidato mora no HOME (ou onde a própria toolchain aponta) e diz como se
    /// refaz — nada entra na lista sem resposta pra "e se eu apagar?".
    #[test]
    fn candidatos_sao_do_home_e_explicam_como_refazer() {
        for (p, _, refaz) in candidatos() {
            assert!(p.is_absolute(), "caminho relativo em cache global: {}", p.display());
            assert!(!refaz.is_empty(), "cache sem instrução de recriação: {}", p.display());
        }
    }

    /// A varredura só devolve o que EXISTE — máquina sem Go não vê cache de Go.
    #[test]
    fn so_reporta_o_que_existe() {
        for a in varrer(0) {
            assert!(a.caminho.is_dir(), "reportou o que não existe: {}", a.caminho.display());
        }
    }
}
