//! DISCO — inventário e limpeza do lixo recriável da máquina.
//!
//! O quê: descobre o que está ocupando espaço e pode ser refeito (artefato de build,
//! cache de toolchain, camada de Docker), agrupa POR DISCO, e apaga com trava. Onde:
//! `schematize disco` no CLI e a tela Disco na GUI.
//!
//! O problema que isto resolve, na letra: uma máquina com um disco só pra projetos e o
//! disco principal — e é o principal que vive enchendo, quase sempre de `target/` de
//! Rust, cache de Go e camada de Docker. Um total geral não ajuda; a pergunta é "o que
//! está enchendo ESTE disco". Por isso tudo aqui é agrupado por ponto de montagem.
//!
//! Piso de segurança, porque isto APAGA:
//!  - lista FECHADA de nomes ([`artefatos`]) — nada de heurística por tamanho/nome;
//!  - só apaga dentro dos diretórios de dev cadastrados ou dos caches globais
//!    conhecidos (`remover` recusa o resto, mesmo se pedirem);
//!  - nunca apaga fonte, e nunca `--volumes` do Docker junto com o resto (volume é
//!    dado, não build);
//!  - nada acontece sem pedido explícito: a varredura é só leitura.

pub mod artefatos;
pub mod caches;
pub mod docker;
pub mod montagem;
pub mod tamanho;

use std::path::{Path, PathBuf};

/// De que espécie é o lixo. Serve pra filtrar e pra explicar o custo de apagar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    RustTarget,
    NodeModules,
    NodeBuild,
    NodeCache,
    GoCache,
    PythonCache,
    PythonVenv,
    CargoCache,
}

impl Tipo {
    /// Rótulo curto pra UI/CLI.
    pub fn rotulo(&self) -> &'static str {
        match self {
            Tipo::RustTarget => "target (Rust)",
            Tipo::NodeModules => "node_modules",
            Tipo::NodeBuild => "build (Node)",
            Tipo::NodeCache => "cache npm",
            Tipo::GoCache => "cache Go",
            Tipo::PythonCache => "cache Python",
            Tipo::PythonVenv => "venv Python",
            Tipo::CargoCache => "cache cargo",
        }
    }

    /// Apagar isto custa REDE (baixar de novo) ou CPU (compilar de novo)?
    /// A UI usa pra ordenar a sugestão: recompilar é mais barato que rebaixar tudo.
    pub fn custa_rede(&self) -> bool {
        matches!(self, Tipo::NodeCache | Tipo::GoCache | Tipo::CargoCache | Tipo::PythonCache)
    }
}

/// Uma coisa apagável, já medida.
#[derive(Debug, Clone)]
pub struct Achado {
    pub caminho: PathBuf,
    pub tipo: Tipo,
    pub bytes: u64,
    /// Dias desde a última modificação — o que separa "build de ontem" de "lixo".
    pub dias_parado: u64,
    /// Disco em que mora (ponto de montagem).
    pub montagem: PathBuf,
    /// Como se refaz, em uma linha. Todo achado tem de responder isso.
    pub refaz: &'static str,
}

/// Mede um diretório e devolve o [`Achado`], ou `None` se não existe / é pequeno demais.
pub(crate) fn medir(caminho: &Path, tipo: Tipo, refaz: &'static str, so_acima_de: u64) -> Option<Achado> {
    if !caminho.is_dir() {
        return None;
    }
    let bytes = tamanho::ocupado(caminho);
    if bytes < so_acima_de {
        return None;
    }
    Some(Achado {
        montagem: montagem::ponto_de_montagem(caminho),
        dias_parado: dias_desde_modificacao(caminho),
        caminho: caminho.to_path_buf(),
        tipo,
        bytes,
        refaz,
    })
}

/// Dias desde a última modificação do diretório. 0 se não der pra ler.
fn dias_desde_modificacao(p: &Path) -> u64 {
    let Ok(md) = std::fs::metadata(p) else { return 0 };
    let Ok(m) = md.modified() else { return 0 };
    let Ok(dur) = std::time::SystemTime::now().duration_since(m) else { return 0 };
    dur.as_secs() / 86_400
}

/// Inventário completo: artefatos dos projetos + caches globais.
///
/// Só LEITURA. `so_acima_de` corta o ruído (um `__pycache__` de 40 KB não interessa).
pub fn inventario(dev_dirs: &[String], so_acima_de: u64) -> Vec<Achado> {
    let mut v = artefatos::varrer(dev_dirs, so_acima_de);
    v.extend(caches::varrer(so_acima_de));
    v.sort_by_key(|x| std::cmp::Reverse(x.bytes));
    v
}

/// Total por DISCO — a visão que responde "o que está enchendo o disco principal".
pub fn por_montagem(achados: &[Achado]) -> Vec<(PathBuf, u64)> {
    let mut mapa: std::collections::BTreeMap<PathBuf, u64> = Default::default();
    for a in achados {
        *mapa.entry(a.montagem.clone()).or_default() += a.bytes;
    }
    let mut v: Vec<(PathBuf, u64)> = mapa.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v
}

/// Total por TIPO — responde "quem é o culpado" (quase sempre `target` e Docker).
pub fn por_tipo(achados: &[Achado]) -> Vec<(Tipo, u64)> {
    let mut mapa: std::collections::BTreeMap<&'static str, (Tipo, u64)> = Default::default();
    for a in achados {
        let e = mapa.entry(a.tipo.rotulo()).or_insert((a.tipo, 0));
        e.1 += a.bytes;
    }
    let mut v: Vec<(Tipo, u64)> = mapa.into_values().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v
}

/// Apaga um achado. A TRAVA está aqui, não em quem chama.
///
/// Só passa o que está dentro de um diretório de dev cadastrado OU é um cache global
/// conhecido. Qualquer outro caminho é recusado — mesmo que alguém peça, mesmo que o
/// caminho exista. Uma função que apaga recursivamente não confia no chamador.
pub fn remover(a: &Achado, dev_dirs: &[String]) -> Result<u64, String> {
    let permitido = artefatos::dentro_de(&a.caminho, dev_dirs)
        || caches::varrer(0).iter().any(|c| c.caminho == a.caminho);
    if !permitido {
        return Err(format!(
            "recusado: {} não está em nenhum diretório de dev cadastrado nem é cache conhecido",
            a.caminho.display()
        ));
    }
    // Recontar antes de apagar: o número do inventário pode estar velho (o build rodou
    // de novo no meio), e o relatório de "liberado" tem de ser honesto.
    let bytes = tamanho::ocupado(&a.caminho);
    std::fs::remove_dir_all(&a.caminho).map_err(|e| format!("{}: {e}", a.caminho.display()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn achado_em(p: &Path, tipo: Tipo) -> Achado {
        Achado {
            caminho: p.to_path_buf(),
            tipo,
            bytes: 1,
            dias_parado: 0,
            montagem: PathBuf::from("/"),
            refaz: "x",
        }
    }

    /// A TRAVA: caminho fora dos dev_dirs é recusado, e o diretório continua lá.
    /// É o teste que impede esta função de virar um `rm -rf` genérico.
    #[test]
    fn remover_recusa_fora_dos_dev_dirs() {
        let base = std::env::temp_dir().join(format!("disco-trava-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let fora = base.join("fora").join("target");
        std::fs::create_dir_all(&fora).unwrap();
        std::fs::write(fora.join("x"), b"dado").unwrap();

        let erro = remover(&achado_em(&fora, Tipo::RustTarget), &[]).unwrap_err();
        assert!(erro.contains("recusado"), "{erro}");
        assert!(fora.is_dir(), "não pode ter apagado nada");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Dentro de um dev_dir cadastrado, apaga e devolve o que liberou.
    #[test]
    fn remover_apaga_dentro_do_dev_dir() {
        let base = std::env::temp_dir().join(format!("disco-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let alvo = base.join("proj").join("target");
        std::fs::create_dir_all(&alvo).unwrap();
        std::fs::write(alvo.join("x"), vec![0u8; 8192]).unwrap();

        let devs = vec![base.display().to_string()];
        let liberado = remover(&achado_em(&alvo, Tipo::RustTarget), &devs).unwrap();
        assert!(liberado >= 8192, "liberou {liberado}");
        assert!(!alvo.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// O agrupamento por disco é o ponto da ferramenta: dois achados no mesmo
    /// ponto de montagem somam numa linha só.
    #[test]
    fn agrupa_por_disco_e_por_tipo() {
        let mut a = achado_em(Path::new("/a/target"), Tipo::RustTarget);
        a.bytes = 100;
        a.montagem = PathBuf::from("/");
        let mut b = achado_em(Path::new("/b/target"), Tipo::RustTarget);
        b.bytes = 50;
        b.montagem = PathBuf::from("/");
        let mut c = achado_em(Path::new("/dados/x/node_modules"), Tipo::NodeModules);
        c.bytes = 70;
        c.montagem = PathBuf::from("/dados");

        let m = por_montagem(&[a.clone(), b.clone(), c.clone()]);
        assert_eq!(m[0], (PathBuf::from("/"), 150), "o disco mais cheio vem primeiro");
        assert_eq!(m[1], (PathBuf::from("/dados"), 70));

        let t = por_tipo(&[a, b, c]);
        assert_eq!(t[0].1, 150);
        assert_eq!(t[0].0, Tipo::RustTarget);
    }

    /// Todo tipo tem rótulo e diz se apagar custa rede — a UI depende disso pra
    /// sugerir o que limpar primeiro.
    #[test]
    fn todo_tipo_se_explica() {
        for t in [Tipo::RustTarget, Tipo::NodeModules, Tipo::NodeBuild, Tipo::NodeCache,
                  Tipo::GoCache, Tipo::PythonCache, Tipo::PythonVenv, Tipo::CargoCache] {
            assert!(!t.rotulo().is_empty());
        }
        assert!(!Tipo::RustTarget.custa_rede(), "target se refaz compilando");
        assert!(Tipo::CargoCache.custa_rede(), "cache se refaz baixando");
    }
}
