//! Em qual DISCO um caminho está.
//!
//! O quê: dado um caminho, devolve o ponto de montagem que o contém. Onde: usado
//! pelo inventário do `disco` pra agrupar o lixo por disco. Só leitura.
//!
//! Por que isto existe: a máquina do dono tem um disco só pra projetos e o disco
//! principal — e é o principal que vive enchendo. Um total geral de "45 GB de
//! artefato" não ajuda em nada se metade está no disco que tem espaço sobrando. A
//! pergunta útil é "o que está enchendo ESTE disco", e pra responder isso é preciso
//! saber onde cada coisa mora.
//!
//! Como: sobe do caminho até a raiz enquanto o número do DISPOSITIVO for o mesmo
//! (`st_dev`). O primeiro diretório cujo pai está noutro dispositivo é o ponto de
//! montagem. É a mesma regra do `df`, só que sem depender do `df` nem parsear tabela.

use std::path::{Path, PathBuf};

/// Número do dispositivo de um caminho (None se não der pra ler).
#[cfg(unix)]
fn dispositivo(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
fn dispositivo(_p: &Path) -> Option<u64> {
    None
}

/// Ponto de montagem que contém `caminho`.
///
/// Fallback pra `/` quando não dá pra determinar (caminho sumiu, sistema sem `st_dev`) —
/// agrupar no lugar errado é melhor que não listar o achado.
pub fn ponto_de_montagem(caminho: &Path) -> PathBuf {
    let mut atual: PathBuf = match caminho.canonicalize() {
        Ok(p) => p,
        Err(_) => caminho.to_path_buf(),
    };
    let Some(dev) = dispositivo(&atual) else {
        return PathBuf::from("/");
    };
    while let Some(pai) = atual.parent() {
        match dispositivo(pai) {
            // O pai está em OUTRO dispositivo: `atual` é onde o disco começa.
            Some(d) if d != dev => return atual,
            Some(_) => atual = pai.to_path_buf(),
            None => return atual,
        }
    }
    PathBuf::from("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A raiz é o ponto de montagem dela mesma — o caso-base do laço.
    #[test]
    fn raiz_e_seu_proprio_ponto() {
        assert_eq!(ponto_de_montagem(Path::new("/")), PathBuf::from("/"));
    }

    /// Um caminho qualquer resolve pra ALGUM ponto que o contém como prefixo.
    /// (Qual é depende da máquina — o que dá pra afirmar é a relação.)
    #[test]
    fn ponto_e_prefixo_do_caminho() {
        let alvo = std::env::temp_dir();
        let ponto = ponto_de_montagem(&alvo);
        let alvo_c = alvo.canonicalize().unwrap_or(alvo);
        assert!(alvo_c.starts_with(&ponto), "{} não começa com {}", alvo_c.display(), ponto.display());
    }

    /// Caminho que não existe não entra em pânico nem devolve vazio.
    #[test]
    fn caminho_inexistente_nao_quebra() {
        let p = ponto_de_montagem(Path::new("/nao/existe/mesmo/aqui"));
        assert!(p.is_absolute());
    }
}
