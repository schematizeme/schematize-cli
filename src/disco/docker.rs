//! DOCKER — quanto ele está ocupando e o que dá pra recuperar.
//!
//! O quê: lê o `docker system df` e expõe as podas por CATEGORIA. Onde: entra no
//! inventário do `disco` como uma linha por categoria. Sem docker instalado/rodando,
//! devolve vazio — não é erro.
//!
//! Por que não tem "limpar tudo": `docker system prune -a --volumes` apaga VOLUME, e
//! volume é dado (banco de dev, upload de teste), não build. Aqui cada poda é uma
//! operação separada, e a de volume é a única que exige confirmação extra — perder
//! imagem custa um `docker pull`, perder volume pode custar o trabalho de alguém.

use crate::util;

/// Uma categoria do `docker system df`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Categoria {
    /// "Images", "Containers", "Local Volumes", "Build Cache".
    pub tipo: String,
    /// Quanto ela ocupa, em bytes.
    pub bytes: u64,
    /// Quanto disso é recuperável, em bytes.
    pub recuperavel: u64,
}

/// O docker está disponível E respondendo? (`docker info` falha com o daemon parado.)
pub fn disponivel() -> bool {
    util::run("docker", &["info", "--format", "{{.ServerVersion}}"]).is_ok()
}

/// Lê o `docker system df`. Vazio se o docker não estiver disponível.
pub fn uso() -> Vec<Categoria> {
    let Ok(saida) = util::run(
        "docker",
        &["system", "df", "--format", "{{.Type}}\t{{.Size}}\t{{.Reclaimable}}"],
    ) else {
        return Vec::new();
    };
    saida.lines().filter_map(parse_linha).collect()
}

/// Parseia UMA linha `Tipo\tTamanho\tRecuperável`. PURA — é onde mora o formato do
/// docker, e é o que quebra quando eles mudam a saída; por isso é testada à parte.
///
/// O campo recuperável vem como `1.2GB (45%)` — o percentual é descartado.
pub fn parse_linha(l: &str) -> Option<Categoria> {
    let mut campos = l.split('\t');
    let tipo = campos.next()?.trim().to_string();
    let bytes = parse_tamanho(campos.next()?.trim())?;
    let rec_bruto = campos.next().unwrap_or("0B");
    let rec = rec_bruto.split_whitespace().next().unwrap_or("0B");
    if tipo.is_empty() {
        return None;
    }
    Some(Categoria { tipo, bytes, recuperavel: parse_tamanho(rec).unwrap_or(0) })
}

/// `1.2GB`, `860.5MB`, `0B` -> bytes. O docker usa base 1000 nessa saída.
pub fn parse_tamanho(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix("TB") {
        (n, 1_000_000_000_000.0)
    } else if let Some(n) = s.strip_suffix("GB") {
        (n, 1_000_000_000.0)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1_000_000.0)
    } else if let Some(n) = s.strip_suffix("kB").or_else(|| s.strip_suffix("KB")) {
        (n, 1_000.0)
    } else if let Some(n) = s.strip_suffix('B') {
        (n, 1.0)
    } else {
        (s, 1.0)
    };
    num.trim().parse::<f64>().ok().map(|v| (v * mult) as u64)
}

/// As podas oferecidas, em ordem de risco crescente.
///
/// `(rótulo, argumentos, destrói dado?)`. O último campo é o que a UI/CLI usa pra
/// exigir confirmação extra — e é por isso que ele existe em vez de um comentário.
pub fn podas() -> Vec<(&'static str, Vec<&'static str>, bool)> {
    vec![
        ("cache de build", vec!["builder", "prune", "-af"], false),
        ("containers parados", vec!["container", "prune", "-f"], false),
        ("imagens sem uso", vec!["image", "prune", "-af"], false),
        ("redes sem uso", vec!["network", "prune", "-f"], false),
        // Volume é DADO. Fica por último e marcado — nunca entra num "limpar tudo".
        ("volumes sem uso (APAGA DADOS)", vec!["volume", "prune", "-f"], true),
    ]
}

/// Roda uma poda pelo rótulo. Devolve a saída do docker.
pub fn podar(rotulo: &str) -> Result<String, String> {
    let (_, args, _) = podas()
        .into_iter()
        .find(|(r, _, _)| *r == rotulo)
        .ok_or_else(|| format!("poda desconhecida: {rotulo}"))?;
    util::run("docker", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tamanho_cobre_as_unidades() {
        assert_eq!(parse_tamanho("0B"), Some(0));
        assert_eq!(parse_tamanho("500B"), Some(500));
        assert_eq!(parse_tamanho("1.5kB"), Some(1_500));
        assert_eq!(parse_tamanho("860.5MB"), Some(860_500_000));
        assert_eq!(parse_tamanho("2.3GB"), Some(2_300_000_000));
    }

    #[test]
    fn parse_linha_do_system_df() {
        let c = parse_linha("Images\t12.4GB\t8.1GB (65%)").unwrap();
        assert_eq!(c.tipo, "Images");
        assert_eq!(c.bytes, 12_400_000_000);
        assert_eq!(c.recuperavel, 8_100_000_000, "o percentual entre parênteses é descartado");
    }

    #[test]
    fn linha_invalida_nao_vira_categoria() {
        assert!(parse_linha("").is_none());
        assert!(parse_linha("só um campo").is_none());
    }

    /// A poda de VOLUME é a única marcada como destrutiva — é o que faz a UI pedir
    /// confirmação extra e o que a mantém fora de qualquer "limpar tudo".
    #[test]
    fn so_volume_e_marcado_destrutivo() {
        let ps = podas();
        let destrutivas: Vec<&str> = ps.iter().filter(|(_, _, d)| *d).map(|(r, _, _)| *r).collect();
        assert_eq!(destrutivas.len(), 1);
        assert!(destrutivas[0].contains("volume"));
        assert!(ps.iter().all(|(_, args, _)| args.contains(&"prune")), "toda poda é um prune");
    }
}
