//! Varredura de ARTEFATOS de build nos diretórios de desenvolvimento.
//!
//! O quê: acha os diretórios que são saída de build (recriáveis) dentro dos projetos.
//! Onde: alimenta o inventário do `disco`. Só leitura — quem apaga é o `mod.rs`, e só
//! com confirmação.
//!
//! O critério de "é lixo": o diretório é RECRIÁVEL por um comando do próprio
//! ecossistema (`cargo build`, `go build`, `npm i`). Nada de heurística por tamanho ou
//! por nome parecido: a lista é fechada e cada entrada diz como se refaz. Apagar
//! errado aqui custa o trabalho de alguém.

use super::{Achado, Tipo};
use std::path::{Path, PathBuf};

/// Profundidade máxima da varredura a partir de cada diretório de dev.
/// O mesmo teto do `projects::scan` — projeto aninhado além disso é exceção.
const PROFUNDIDADE: usize = 5;

/// Diretórios em que NUNCA entramos (ou reentraríamos no próprio lixo, e a varredura
/// de um `node_modules` com 40 mil pastas leva mais tempo que o resto todo).
const NAO_ENTRA: &[&str] = &["node_modules", "target", ".git", ".venv", "vendor", ".next"];

/// Nome de diretório -> (tipo, como se refaz). Lista FECHADA, por design.
fn catalogo() -> Vec<(&'static str, Tipo, &'static str)> {
    vec![
        ("target", Tipo::RustTarget, "cargo build"),
        ("node_modules", Tipo::NodeModules, "npm install"),
        (".next", Tipo::NodeBuild, "next build"),
        ("dist", Tipo::NodeBuild, "build do projeto"),
        ("__pycache__", Tipo::PythonCache, "recriado ao rodar"),
        (".venv", Tipo::PythonVenv, "python -m venv .venv"),
    ]
}

/// Varre os diretórios de dev e devolve os artefatos encontrados.
///
/// `so_acima_de` corta o ruído: um `__pycache__` de 40 KB não interessa a ninguém.
pub fn varrer(dev_dirs: &[String], so_acima_de: u64) -> Vec<Achado> {
    let mut achados = Vec::new();
    for d in dev_dirs {
        desce(Path::new(d), 0, so_acima_de, &mut achados);
    }
    achados.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    achados
}

/// Desce recursivamente procurando artefatos. Ao ACHAR um, não desce dentro dele.
fn desce(dir: &Path, nivel: usize, so_acima_de: u64, out: &mut Vec<Achado>) {
    if nivel > PROFUNDIDADE {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if !ft.is_dir() || ft.is_symlink() {
            continue;
        }
        let caminho = e.path();
        let Some(nome) = caminho.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some((_, tipo, refaz)) = catalogo().into_iter().find(|(n, _, _)| *n == nome) {
            if let Some(a) = super::medir(&caminho, tipo, refaz, so_acima_de) {
                out.push(a);
            }
            continue; // não desce dentro do artefato
        }
        if NAO_ENTRA.contains(&nome) || nome.starts_with('.') && nome != ".config" {
            continue;
        }
        desce(&caminho, nivel + 1, so_acima_de, out);
    }
}

/// Só os artefatos parados há pelo menos `dias` — o alvo de "build defeituosa que
/// ninguém mais usa". Um `target/` do projeto de ontem não é lixo; o de seis meses é.
pub fn parados_ha(achados: &[Achado], dias: u64) -> Vec<Achado> {
    achados.iter().filter(|a| a.dias_parado >= dias).cloned().collect()
}

/// Caminho do artefato pertence a algum dos diretórios de dev? Guarda-corpo do
/// `remover`: nunca apagamos nada fora do que foi varrido.
pub fn dentro_de(caminho: &Path, dev_dirs: &[String]) -> bool {
    let alvo = caminho.canonicalize().unwrap_or_else(|_| caminho.to_path_buf());
    dev_dirs.iter().any(|d| {
        let base = PathBuf::from(d).canonicalize().unwrap_or_else(|_| PathBuf::from(d));
        alvo.starts_with(&base) && alvo != base
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cria(base: &Path, rel: &str, bytes: usize) {
        let p = base.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![0u8; bytes]).unwrap();
    }

    /// Acha o `target/` do projeto e NÃO desce dentro dele (senão cada subpasta
    /// viraria um achado e a conta sairia inflada).
    #[test]
    fn acha_target_sem_descer_dentro() {
        let base = std::env::temp_dir().join(format!("art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        cria(&base, "proj/src/main.rs", 100);
        cria(&base, "proj/target/debug/x", 20_000);
        cria(&base, "proj/target/release/y", 20_000);

        let a = varrer(&[base.display().to_string()], 0);
        assert_eq!(a.len(), 1, "um achado só (o target), veio {a:?}");
        assert_eq!(a[0].tipo, Tipo::RustTarget);
        assert!(a[0].caminho.ends_with("proj/target"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Fonte NUNCA é achado — é o piso de segurança da varredura.
    #[test]
    fn nunca_reporta_codigo_fonte() {
        let base = std::env::temp_dir().join(format!("art-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        cria(&base, "proj/src/main.rs", 50_000);
        cria(&base, "proj/lib/util.rs", 50_000);
        let a = varrer(&[base.display().to_string()], 0);
        assert!(a.is_empty(), "não pode reportar fonte: {a:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// O corte por tamanho tira o ruído.
    #[test]
    fn corte_por_tamanho() {
        let base = std::env::temp_dir().join(format!("art-min-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        cria(&base, "p/__pycache__/x.pyc", 100);
        assert!(varrer(&[base.display().to_string()], 0).len() == 1);
        assert!(varrer(&[base.display().to_string()], 10_000_000).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// `dentro_de` é o guarda-corpo do apagar: fora dos dev_dirs, não passa.
    #[test]
    fn guarda_corpo_do_remover() {
        let base = std::env::temp_dir().join(format!("art-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("proj/target")).unwrap();
        let devs = vec![base.display().to_string()];
        assert!(dentro_de(&base.join("proj/target"), &devs));
        assert!(!dentro_de(Path::new("/usr/lib"), &devs), "fora do dev_dir não passa");
        assert!(!dentro_de(&base, &devs), "o próprio dev_dir não é alvo");
        let _ = std::fs::remove_dir_all(&base);
    }
}
