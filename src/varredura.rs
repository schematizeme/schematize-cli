//! Varredura de arquivo grande por trechos (streaming), sem carregá-lo inteiro em memória.
//!
//! Existe porque o `doctor` lia o binário da GUI (dezenas de MB) com `fs::read` só para
//! procurar duas ou três palavras: o pico de RSS era proporcional ao tamanho do arquivo.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Tamanho do trecho lido por vez (64 KiB): o pico de memória da varredura é esse valor
/// mais a sobreposição, independente do tamanho do arquivo.
const TRECHO: usize = 64 * 1024;

/// O quê: diz, para cada agulha, se ela ocorre em algum ponto do arquivo em `caminho`.
/// Lê em trechos de 64 KiB e mantém a cauda (maior agulha - 1 bytes) entre trechos, para
/// achar ocorrências que cruzam a fronteira. Comportamento idêntico ao `windows().any()`
/// sobre o arquivo inteiro. Agulha vazia é sempre `true`.
/// Onde: `doctor` (checagem do flavor do `schematize-gui`: Slint vs egui).
pub fn contem_agulhas(caminho: &Path, agulhas: &[&[u8]]) -> io::Result<Vec<bool>> {
    varre(File::open(caminho)?, agulhas)
}

/// O quê: núcleo de `contem_agulhas` sobre qualquer `Read` (testável sem disco).
/// Onde: chamado só por `contem_agulhas` e pelos testes deste módulo.
fn varre<R: Read>(mut leitor: R, agulhas: &[&[u8]]) -> io::Result<Vec<bool>> {
    let mut achou: Vec<bool> = agulhas.iter().map(|a| a.is_empty()).collect();
    let cauda_max = agulhas.iter().map(|a| a.len()).max().unwrap_or(0).saturating_sub(1);
    let mut buf: Vec<u8> = Vec::with_capacity(TRECHO + cauda_max);
    let mut trecho = vec![0u8; TRECHO];
    loop {
        let n = leitor.read(&mut trecho)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&trecho[..n]);
        for (i, a) in agulhas.iter().enumerate() {
            if !achou[i] && buf.windows(a.len()).any(|w| w == *a) {
                achou[i] = true;
            }
        }
        if achou.iter().all(|&b| b) {
            break;
        }
        let manter = cauda_max.min(buf.len());
        buf.drain(..buf.len() - manter);
    }
    Ok(achou)
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Leitor que entrega no máximo `passo` bytes por `read`, para forçar fronteiras.
    struct Fatiado<'a>(&'a [u8], usize);
    impl Read for Fatiado<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = self.0.len().min(self.1).min(out.len());
            out[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    #[test]
    fn acha_agulha_que_cruza_a_fronteira() {
        let dados = b"xxxxxsl";
        let mais = b"intyyyy";
        let tudo = [&dados[..], &mais[..]].concat();
        for passo in 1..=8 {
            let r = varre(Fatiado(&tudo, passo), &[b"slint", b"egui"]).unwrap();
            assert_eq!(r, vec![true, false], "passo {passo}");
        }
    }

    #[test]
    fn agulha_ausente_vazia_e_arquivo_vazio() {
        assert_eq!(varre(Fatiado(b"abc", 2), &[b"zzz"]).unwrap(), vec![false]);
        assert_eq!(varre(Fatiado(b"", 2), &[b"a", b""]).unwrap(), vec![false, true]);
    }

    #[test]
    fn acha_alem_do_primeiro_trecho_grande() {
        let truncado = [vec![0u8; TRECHO * 3], b"efram".to_vec()].concat();
        assert_eq!(varre(&truncado[..], &[b"eframe"]).unwrap(), vec![false]);
        let mut w = vec![0u8; TRECHO * 3];
        w.extend_from_slice(b"eframe");
        assert_eq!(varre(&w[..], &[b"slint", b"eframe"]).unwrap(), vec![false, true]);
    }
}
