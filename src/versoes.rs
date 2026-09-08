//! Qual é a última versão de um repositório da casa.
//!
//! **O quê:** pergunta ao GitHub a versão publicada de um repo — pelo arquivo cru
//! ([`latest_version_raw`], o caminho primário) ou pela API de releases
//! ([`latest_release_tag`], fallback de diagnóstico).
//!
//! **Onde:** `upgrade`, `selfupdate`, `doctor` e `debug` perguntam pela versão da PRÓPRIA
//! CLI; `skills` pergunta pela de cada skill.
//!
//! ## Por que isto saiu do `skills.rs`
//!
//! Morava lá por acidente de história, e o próprio código dizia: `latest_version_raw` tinha
//! um ramo `if is_cli { "Cargo.toml" } else { "VERSION" }`. Uma função **de skills** com um
//! caso especial *"a não ser que seja a CLI"* é uma função no módulo errado — e era ela que
//! fazia o `upgrade`, o `selfupdate` e o `doctor` aparecerem como "consumidores de skills"
//! na medição do ADR-0012, sem que nenhum deles quisesse uma skill.
//!
//! Movê-la não é enfeite: é o que faz a extração do `schematize-skills` deixar de mexer
//! nesses três arquivos.
//!
//! ## Por que a parte pura está separada da rede
//!
//! Enquanto o `curl` estava no meio da função, ela não tinha teste nenhum — nem para o
//! `Cargo.toml` sem `[package]`, nem para a tag com `v`, nem para o corpo vazio. Com
//! [`parse_versao`] e [`parse_tag`] puros, cada um desses casos é uma linha de teste.

use crate::registry;
use crate::util;

/// Timeout curto (s): saber a versão é conveniência; travar a UI por causa dela, não.
const NET_TIMEOUT: &str = "8";

/// O `User-Agent` que o GitHub exige — sem ele a API responde 403.
const UA: &str = "User-Agent: schematize-cli";

/// **O quê:** a versão publicada de um repo, lida do arquivo CRU em `main`.
///
/// **Onde:** o caminho primário de detecção — `upgrade`, `selfupdate`, `doctor`, `status` e
/// `skills`.
///
/// **Por que raw e não a API:** a API do GitHub é limitada a 60 requisições por hora por IP,
/// e esse teto zerava a detecção de versão em quem usava o app de verdade. O `raw` tem cache
/// curto e limite muito maior.
///
/// Rede: nunca panica; qualquer falha vira `None`.
pub fn latest_version_raw(repo: &str) -> Option<String> {
    let is_cli = repo == CLI_REPO;
    let file = if is_cli { "Cargo.toml" } else { "VERSION" };
    let url = format!("https://raw.githubusercontent.com/{}/{}/main/{}", registry::ORG, repo, file);
    let body = util::run("curl", &["-sfL", "-m", NET_TIMEOUT, "-H", UA, &url]).ok()?;
    parse_versao(&body, is_cli)
}

/// O repo da própria CLI — o único cuja versão mora no `Cargo.toml` e não num `VERSION`.
pub const CLI_REPO: &str = "schematize-cli";

/// **O quê:** extrai a versão do corpo do arquivo. PURA. **Onde:** [`latest_version_raw`].
///
/// `cargo = true` lê a linha `version = "x.y.z"` do `Cargo.toml`; senão o arquivo inteiro é a
/// versão. Devolve `None` quando não há o que ler — um corpo vazio ou uma página de erro do
/// GitHub NÃO viram uma "versão" que faria o app se achar desatualizado para sempre.
pub fn parse_versao(body: &str, cargo: bool) -> Option<String> {
    if cargo {
        // A PRIMEIRA linha `version =` é a do `[package]`. As de dependência vêm depois, e
        // pegar uma delas faria o app comparar a própria versão com a de uma lib.
        return body
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("version") && l.contains('='))
            .and_then(|l| l.split('"').nth(1))
            .map(str::to_string);
    }
    let v = body.trim();
    // Tem que PARECER versão inteira, não só começar com dígito.
    //
    // A guarda anterior era `começa com dígito`, e "404: Not Found" começa com dígito — ela
    // deixava a página de erro do GitHub virar "a última versão". Na prática o `curl -f`
    // costuma barrar antes (falha o processo em erro de HTTP), mas "costuma" não é guarda:
    // basta um proxy que responda 200 com corpo de erro, ou um VERSION com lixo, para o app
    // passar a anunciar a atualização "404" para sempre — `semver_lt` leria 404 como major.
    let versao = !v.is_empty()
        && v.starts_with(|c: char| c.is_ascii_digit())
        && v.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c.is_ascii_alphabetic());
    versao.then(|| v.to_string())
}

/// **O quê:** a última versão pela API de releases do GitHub, sem o `v` do prefixo.
///
/// **Onde:** fallback e diagnóstico (`debug`) — NÃO é o caminho primário, porque a API é
/// limitada a 60/h/IP.
pub fn latest_release_tag(repo: &str) -> Option<String> {
    let url = format!("https://api.github.com/repos/{}/{}/releases/latest", registry::ORG, repo);
    let body =
        util::run("curl", &["-sfL", "-H", "Accept: application/vnd.github+json", "-H", UA, &url])
            .ok()?;
    parse_tag(&body)
}

/// **O quê:** extrai `tag_name` do JSON de release, sem o `v`. PURA.
/// **Onde:** [`latest_release_tag`].
pub fn parse_tag(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(v.get("tag_name")?.as_str()?.trim_start_matches('v').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A primeira `version =` é a do pacote. Se o parser pegasse a de uma dependência, o app
    /// compararia a própria versão com a de uma lib e se acharia desatualizado para sempre.
    #[test]
    fn cargo_toml_pega_a_versao_do_pacote_e_nao_a_da_dependencia() {
        let toml = "[package]\nname = \"schematize\"\nversion = \"0.62.0\"\n\n\
                    [dependencies]\nserde = { version = \"1.0.200\" }\n";
        assert_eq!(parse_versao(toml, true).as_deref(), Some("0.62.0"));
    }

    /// O `raw` responde 404 com CORPO — e um corpo que não começa com dígito não é versão.
    /// Sem esta guarda, "404: Not Found" viraria a "última versão" e o app se declararia
    /// desatualizado a cada consulta.
    #[test]
    fn pagina_de_erro_do_github_nao_vira_versao() {
        // "404: Not Found" COMEÇA com dígito — era assim que passava pela guarda antiga.
        assert_eq!(parse_versao("404: Not Found\n", false), None);
        assert_eq!(parse_versao("<!DOCTYPE html>", false), None);
        assert_eq!(parse_versao("", false), None);
        assert_eq!(parse_versao("   \n", false), None);
    }

    /// Um `VERSION` normal é o arquivo inteiro, com o `\n` do fim aparado.
    #[test]
    fn arquivo_version_e_o_conteudo_aparado() {
        assert_eq!(parse_versao("1.10.0\n", false).as_deref(), Some("1.10.0"));
    }

    /// A guarda não pode ser tão apertada que recuse versão legítima: pré-release e build
    /// metadata são semver válido e existem nos repos da casa.
    #[test]
    fn pre_release_continua_valendo() {
        assert_eq!(parse_versao("1.10.0-rc1\n", false).as_deref(), Some("1.10.0-rc1"));
        assert_eq!(parse_versao("2.0.0-beta.1", false).as_deref(), Some("2.0.0-beta.1"));
    }

    /// `Cargo.toml` sem `version` nenhuma não inventa resposta.
    #[test]
    fn cargo_toml_sem_versao_devolve_none() {
        assert_eq!(parse_versao("[package]\nname = \"x\"\n", true), None);
    }

    /// A tag vem `v1.2.3` e o resto do app compara sem o `v`. Com ele, `semver_lt` compararia
    /// "v1.2.3" com "1.2.3" e diria que há atualização a cada execução.
    #[test]
    fn tag_perde_o_v_do_prefixo() {
        assert_eq!(parse_tag(r#"{"tag_name":"v1.2.3"}"#).as_deref(), Some("1.2.3"));
        assert_eq!(parse_tag(r#"{"tag_name":"1.2.3"}"#).as_deref(), Some("1.2.3"));
    }

    /// Resposta de erro da API (ou JSON quebrado) vira `None`, não panica.
    #[test]
    fn json_sem_tag_ou_quebrado_devolve_none() {
        assert_eq!(parse_tag(r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_tag("não é json"), None);
    }
}
