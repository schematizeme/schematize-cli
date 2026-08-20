//! Tamanho de uma árvore de diretórios, e bytes em unidade legível.
//!
//! O quê: soma o espaço realmente ocupado por um diretório. Onde: o inventário do
//! `disco` chama isto pra cada achado. Só leitura.
//!
//! Nota sobre o número: somamos os BLOCOS ocupados (`st_blocks`), não o tamanho
//! lógico dos arquivos. É o que o `du` faz, e é o que corresponde ao espaço que volta
//! pro disco ao apagar — arquivo esparso e hardlink mentem no tamanho lógico.

use std::path::Path;

/// Espaço ocupado por uma árvore, em bytes.
///
/// Best-effort: o que não der pra ler (permissão, corrida com quem apaga) conta zero.
/// Este número serve pra decidir o que limpar, não pra auditoria contábil.
/// Não segue symlink — senão um link pra `/` faria a varredura sair andando pela
/// máquina inteira e contar o mesmo byte várias vezes.
pub fn ocupado(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        if md.file_type().is_symlink() {
            continue;
        }
        if md.is_dir() {
            total += ocupado(&e.path());
        } else {
            total += blocos(&md);
        }
    }
    total
}

/// Bytes ocupados por um arquivo (blocos de 512 B, como o `st_blocks` do Unix).
#[cfg(unix)]
fn blocos(md: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    md.blocks() * 512
}

#[cfg(not(unix))]
fn blocos(md: &std::fs::Metadata) -> u64 {
    md.len()
}

/// Bytes em unidade legível: `1,4 GB`, `860 MB`, `12 KB`.
pub fn legivel(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB).replace('.', ",")
    } else if b >= MB {
        format!("{:.0} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legivel_escolhe_a_unidade() {
        assert_eq!(legivel(512), "512 B");
        assert_eq!(legivel(2048), "2 KB");
        assert_eq!(legivel(5 * 1024 * 1024), "5 MB");
        assert_eq!(legivel(3 * 1024 * 1024 * 1024), "3,0 GB");
    }

    /// Soma a árvore inteira, incluindo subdiretório.
    #[test]
    fn soma_a_arvore() {
        let base = std::env::temp_dir().join(format!("tam-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::write(base.join("a"), vec![0u8; 8192]).unwrap();
        std::fs::write(base.join("sub").join("b"), vec![0u8; 8192]).unwrap();
        let t = ocupado(&base);
        assert!(t >= 16384, "esperava >= 16 KB, veio {t}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Diretório que não existe é zero, não pânico.
    #[test]
    fn inexistente_e_zero() {
        assert_eq!(ocupado(Path::new("/nao/existe/mesmo")), 0);
    }
}
