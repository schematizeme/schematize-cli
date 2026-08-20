//! Qual versão de cada skill foi APLICADA A ESTE PROJETO.
//!
//! O quê: um registro por projeto de "a skill X, na versão Y, já rodou aqui". Onde:
//! `overflow skills applied` no CLI e o aviso/botão da tela do projeto na GUI.
//!
//! ## O problema
//!
//! Skill instalada e skill aplicada são coisas diferentes, e só a primeira era
//! rastreada. `~/.claude/skills/<slug>/VERSION` diz qual versão está na MÁQUINA;
//! nada dizia qual versão moldou os preceitos DESTE projeto. Então quando uma skill
//! evolui — e evoluem: a engineering foi de 0.15 a 0.20 em semanas —, os projetos
//! antigos seguem com os preceitos da versão velha e ninguém percebe. O sintoma é
//! sutil e caro: dois projetos da mesma casa com padrões diferentes, sem nenhum aviso.
//!
//! ## O registro
//!
//! Mora em `<projeto>/.overflow/skills.json` — dentro do projeto, porque é um fato
//! sobre o projeto, não sobre a máquina. Versionável junto com o código, o que também
//! responde "com que padrões isto foi feito?" pra quem chegar depois.
//!
//! ## Quem escreve
//!
//! [`marcar`], e só ela — chamada pelo agente ao TERMINAR de aplicar a skill. Marcar
//! ao disparar seria mentira: o agente pode falhar no meio, e o registro diria que
//! está aplicado quando não está. Melhor um projeto que se diz desatualizado sem
//! estar do que o contrário.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// O que se sabe sobre uma skill aplicada aqui.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Aplicada {
    /// Versão da skill que rodou neste projeto.
    pub versao: String,
    /// Epoch de quando rodou.
    pub ts: u64,
}

/// Como uma skill deste projeto está em relação à instalada na máquina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Estado {
    /// Aplicada na mesma versão que está instalada.
    Atual,
    /// Instalada é mais nova que a aplicada — os preceitos do projeto ficaram pra trás.
    Desatualizada { aplicada: String, instalada: String },
    /// Instalada na máquina, mas nunca aplicada neste projeto.
    NuncaAplicada { instalada: String },
}

/// `<projeto>/.overflow/skills.json` (ou o nome anterior do dir, se for o em uso).
pub fn arquivo(root: &Path) -> PathBuf {
    crate::paths::schematize_dir_at(root).join("skills.json")
}

/// Lê o registro. Ausente ou corrompido = vazio; isto nunca deve impedir de trabalhar.
pub fn aplicadas(root: &Path) -> BTreeMap<String, Aplicada> {
    std::fs::read_to_string(arquivo(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Registra que `slug` acabou de ser aplicada na versão `versao`.
///
/// Escrita atômica (temporário + rename): o registro é lido por outras ferramentas e
/// um arquivo pela metade viraria "nunca aplicada" — pior que não registrar.
pub fn marcar(root: &Path, slug: &str, versao: &str) -> Result<(), String> {
    let versao = versao.trim();
    if versao.is_empty() {
        return Err("versão vazia".into());
    }
    let mut m = aplicadas(root);
    m.insert(
        slug.to_string(),
        Aplicada {
            versao: versao.to_string(),
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        },
    );
    let corpo = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
    crate::overdev::trava::escreve_atomico(&arquivo(root), &corpo)
}

/// Compara o aplicado com o instalado. PURO — a I/O fica com quem chama, e é o que
/// permite testar a decisão (que é a parte que pode errar) sem tocar o disco.
pub fn comparar(aplicada: Option<&Aplicada>, instalada: Option<&str>) -> Option<Estado> {
    let instalada = instalada?.trim();
    if instalada.is_empty() {
        return None;
    }
    match aplicada {
        None => Some(Estado::NuncaAplicada { instalada: instalada.to_string() }),
        Some(a) if a.versao == instalada => Some(Estado::Atual),
        Some(a) => Some(Estado::Desatualizada {
            aplicada: a.versao.clone(),
            instalada: instalada.to_string(),
        }),
    }
}

/// Estado de TODAS as skills instaladas na máquina, do ponto de vista deste projeto.
///
/// Parte das instaladas, não das registradas: uma skill instalada depois do projeto
/// nascer também é uma resposta útil ("existe isto e você nunca rodou aqui").
pub fn estado_do_projeto(root: &Path) -> Vec<(String, Estado)> {
    let reg = aplicadas(root);
    let mut v: Vec<(String, Estado)> = crate::registry::catalog()
        .iter()
        .filter_map(|it| {
            let inst = crate::skills::installed_version(it)?;
            let e = comparar(reg.get(&it.slug), Some(&inst))?;
            Some((it.slug.clone(), e))
        })
        .collect();
    // O que exige ação primeiro: desatualizada, depois nunca aplicada, depois em dia.
    v.sort_by_key(|(slug, e)| {
        let p = match e {
            Estado::Desatualizada { .. } => 0,
            Estado::NuncaAplicada { .. } => 1,
            Estado::Atual => 2,
        };
        (p, slug.clone())
    });
    v
}

/// As que ficaram pra trás — o conjunto que motiva "rodar de novo".
pub fn desatualizadas(root: &Path) -> Vec<(String, String, String)> {
    estado_do_projeto(root)
        .into_iter()
        .filter_map(|(slug, e)| match e {
            Estado::Desatualizada { aplicada, instalada } => Some((slug, aplicada, instalada)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(v: &str) -> Aplicada {
        Aplicada { versao: v.into(), ts: 0 }
    }

    /// A decisão inteira, sem tocar disco. Os três estados e o caso "nem instalada".
    #[test]
    fn comparacao_cobre_os_estados() {
        assert_eq!(comparar(Some(&ap("0.20.0")), Some("0.20.0")), Some(Estado::Atual));
        assert_eq!(
            comparar(Some(&ap("0.15.0")), Some("0.20.0")),
            Some(Estado::Desatualizada { aplicada: "0.15.0".into(), instalada: "0.20.0".into() })
        );
        assert_eq!(
            comparar(None, Some("0.20.0")),
            Some(Estado::NuncaAplicada { instalada: "0.20.0".into() })
        );
        // Skill não instalada na máquina não gera estado nenhum — não há o que rodar.
        assert_eq!(comparar(Some(&ap("0.15.0")), None), None);
        assert_eq!(comparar(None, None), None);
    }

    /// Versão DIFERENTE conta como desatualizada mesmo se for menor: o que importa é
    /// "os preceitos deste projeto batem com a skill que está na máquina?". Um
    /// downgrade da skill também descasa, e mentir que está atual seria pior.
    #[test]
    fn versao_diferente_e_diferente_nos_dois_sentidos() {
        let e = comparar(Some(&ap("0.30.0")), Some("0.20.0"));
        assert!(matches!(e, Some(Estado::Desatualizada { .. })));
    }

    /// Ida e volta no disco, e o registro sobrevive a reescrita.
    #[test]
    fn marca_e_le_de_volta() {
        let root = std::env::temp_dir().join(format!("ovf-skp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        assert!(aplicadas(&root).is_empty());
        marcar(&root, "engineering", "0.20.0").unwrap();
        marcar(&root, "web", "1.7.0").unwrap();
        let m = aplicadas(&root);
        assert_eq!(m.len(), 2);
        assert_eq!(m["engineering"].versao, "0.20.0");

        // Reaplicar sobrescreve a entrada, não duplica.
        marcar(&root, "engineering", "0.21.0").unwrap();
        let m = aplicadas(&root);
        assert_eq!(m.len(), 2);
        assert_eq!(m["engineering"].versao, "0.21.0");
        assert_eq!(m["web"].versao, "1.7.0", "não mexeu nas outras");

        assert!(marcar(&root, "x", "  ").is_err(), "versão vazia é recusada");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Registro corrompido não impede de trabalhar — devolve vazio, e o projeto
    /// aparece como "nunca aplicada" em vez de o comando quebrar.
    #[test]
    fn registro_corrompido_nao_derruba() {
        let root = std::env::temp_dir().join(format!("ovf-skp-corr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(arquivo(&root).parent().unwrap()).unwrap();
        std::fs::write(arquivo(&root), "{ isto não é json").unwrap();
        assert!(aplicadas(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
