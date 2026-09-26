//! PONTE com o **schematize-skills** — descoberta e leitura, nunca acoplamento.
//!
//! **O quê:** pergunta ao binário das skills o que ele sabe: catálogo com o estado de cada uma,
//! o que está instalado, e os botões que elas declaram.
//!
//! **Onde:** o `status`, o `debug`, o relatório de diagnóstico e as notificações.
//!
//! ## As três regras desta ponte, iguais às do `deployerlink`
//!
//! **1. A ausência do app nunca derruba o schematize** (piso 10). Todo caminho aqui devolve
//! lista vazia como *estado*, não como erro. O `status` mostra o resto, as notificações saem
//! sem as de skill, e o app continua inteiro.
//!
//! **2. Subprocesso, não biblioteca.** Ligar os dois crates faria o hub compilar o domínio de
//! skills junto — e aí não haveria dois apps, haveria um monólito com dois nomes. É o que o
//! ADR-0012 F4 decidiu evitar.
//!
//! **3. UMA leitura por pergunta.** O `status --json` responde "o que eu tenho" sem rede; o
//! `list --json` responde "o que existe e em que pé está", e esse custa rede. Quem chama
//! escolhe — e é por isso que são dois documentos e não um.

/// Nome do binário das skills no `$PATH`.
pub const BIN: &str = "schematize-skills";

/// Uma skill do catálogo, no mínimo que os consumidores deste crate usam.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Skill {
    pub slug: String,
    /// `em_dia`, `tem_atualizacao`, `nao_instalada`, `fork`, `desconhecida`.
    pub situacao: String,
    /// Vazio quando não está instalada.
    pub instalada: String,
    /// Vazio quando não deu para saber.
    pub ultima: String,
}

/// **O quê:** roda um subcomando do app e devolve o stdout. `None` se ele não responder.
fn perguntar(args: &[&str]) -> Option<String> {
    let bin = crate::agentrun::resolve_bin(BIN)?;
    let o = std::process::Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// **O quê:** o catálogo com o estado de cada skill. Lista VAZIA sem o app.
///
/// **Onde:** o `status`, o `debug` e as notificações.
///
/// **Custa REDE**, porque o estado de cada skill inclui a última versão publicada. Quem só
/// quer saber o que está instalado usa [`instaladas`], que não sai da máquina.
pub fn catalogo() -> Vec<Skill> {
    perguntar(&["list", "--json"]).map(|t| interpretar(&t)).unwrap_or_default()
}

/// **O quê:** quantas skills estão instaladas. `None` sem o app.
///
/// **Onde:** o relatório de diagnóstico.
///
/// **`None` e `Some(0)` são coisas diferentes**, e o relatório diz as duas: "0 skills" sobre
/// uma máquina que só não tem o app instalado mandaria alguém procurar o problema errado.
pub fn instaladas() -> Option<usize> {
    let t = perguntar(&["status", "--json"])?;
    let v: serde_json::Value = serde_json::from_str(&t).ok()?;
    Some(v.get("instaladas")?.as_array()?.len())
}

/// **O quê:** o catálogo a partir do TEXTO. PURA, e é onde os testes entram.
///
/// **Onde:** [`catalogo`]. Separada porque o comportamento com documento truncado, de outra
/// versão do app ou hostil tem de ser afirmável sem ter o app instalado na máquina de quem
/// roda a suíte.
pub fn interpretar(texto: &str) -> Vec<Skill> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(texto) else { return Vec::new() };
    let Some(itens) = v.get("skills").and_then(|s| s.as_array()) else { return Vec::new() };
    itens
        .iter()
        .filter_map(|s| {
            let slug = s.get("slug")?.as_str()?.trim();
            // Skill sem slug não é skill: ela não pode ser nomeada numa notificação nem
            // procurada no catálogo, e uma linha em branco na tela é pior que a ausência.
            if slug.is_empty() {
                return None;
            }
            let txt = |c: &str| s.get(c).and_then(|x| x.as_str()).unwrap_or_default().to_string();
            Some(Skill {
                slug: slug.to_string(),
                situacao: txt("situacao"),
                instalada: txt("instalada"),
                ultima: txt("ultima"),
            })
        })
        .collect()
}

impl Skill {
    /// **O quê:** esta skill pede atualização?
    ///
    /// **Onde:** as notificações.
    ///
    /// **Fork NÃO pede**, e é o ponto: uma skill editada compara e mescla. Notificar "há
    /// atualização" sobre um fork é convidar a apagar o trabalho de quem editou.
    pub fn pede_atualizacao(&self) -> bool {
        self.situacao == "tem_atualizacao"
    }
}

/// **O quê:** manda o app ATUALIZAR uma skill. `Err` com o motivo quando não dá.
///
/// **Onde:** o agente de fundo, que aplica as atualizações que encontrou.
///
/// **Aqui o erro é ERRO, e não lista vazia.** As leituras degradam em silêncio porque uma tela
/// sem skills ainda é uma tela; uma atualização que não aconteceu tem de aparecer na
/// notificação de resultado — "pronto" sobre uma falha é a mentira mais cara deste fluxo.
pub fn atualizar(slug: &str) -> Result<(), String> {
    let bin = crate::agentrun::resolve_bin(BIN)
        .ok_or_else(|| format!("`{BIN}` não está instalado — é ele que atualiza skills"))?;
    let o = std::process::Command::new(bin)
        .args(["update", slug])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("não consegui executar `{BIN}`: {e}"))?;
    if o.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&o.stderr);
    let err = err.trim();
    Err(if err.is_empty() { format!("`{BIN} update {slug}` falhou") } else { err.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{"skills":[
      {"slug":"rust","situacao":"em_dia","instalada":"1.11.0","ultima":"1.11.0"},
      {"slug":"web","situacao":"tem_atualizacao","instalada":"1.15.0","ultima":"1.16.0"},
      {"slug":"qa","situacao":"fork","instalada":"0.9.0","ultima":"1.0.0"},
      {"slug":"seo","situacao":"nao_instalada","instalada":null,"ultima":"0.3.0"}]}"#;

    #[test]
    fn le_o_catalogo() {
        let s = interpretar(DOC);
        assert_eq!(s.len(), 4);
        assert_eq!(s[1].slug, "web");
        assert_eq!(s[1].ultima, "1.16.0");
        assert_eq!(s[3].instalada, "", "`null` vira vazio");
    }

    /// **Só uma pede atualização, e o FORK não é ela.** Notificar "há atualização" sobre um
    /// fork é convidar a apagar o trabalho de quem editou.
    #[test]
    fn fork_nao_pede_atualizacao() {
        let s = interpretar(DOC);
        let pedem: Vec<&str> =
            s.iter().filter(|x| x.pede_atualizacao()).map(|x| x.slug.as_str()).collect();
        assert_eq!(pedem, ["web"]);
    }

    /// Skill sem slug é descartada: ela não pode ser nomeada numa notificação nem procurada no
    /// catálogo, e uma linha em branco na tela é pior que a ausência.
    #[test]
    fn skill_sem_slug_e_descartada() {
        let s = interpretar(r#"{"skills":[{"situacao":"em_dia"},{"slug":"  "},{"slug":"ok"}]}"#);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].slug, "ok");
    }

    /// **Documento ruim vira lista vazia, nunca pânico.** Sem o app, as notificações saem sem
    /// as de skill e o resto continua (piso 10).
    #[test]
    fn documento_ruim_vira_lista_vazia() {
        let fundo = format!("{}{}", "[".repeat(3000), "]".repeat(3000));
        for lixo in ["", "null", "[]", "0", "{ nao e json", "\u{0}", &fundo, r#"{"skills":7}"#] {
            assert!(interpretar(lixo).is_empty(), "{lixo:?}");
        }
    }
}
