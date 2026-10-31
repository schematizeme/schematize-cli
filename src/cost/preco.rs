//! Tabela de preços: `pricing.toml` embutido + override do usuário + override do projeto.
//!
//! Modelo sem preço dá custo `None` (nunca inventado) — quem chama transforma em aviso.

use super::Tokens;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// O `pricing.toml` que vai dentro do binário.
const EMBUTIDO: &str = include_str!("pricing.toml");

/// Preço de um modelo, US$ por 1M de tokens.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Preco {
    pub input: f64,
    pub output: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
    #[serde(default)]
    pub vigente_desde: String,
    #[serde(default)]
    pub fonte: String,
    /// `false` = não conferido na fonte oficial. Padrão `false`: na dúvida, não confirmado.
    #[serde(default)]
    pub confirmado: bool,
    #[serde(default)]
    pub nota: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Arquivo {
    #[serde(default)]
    versao: String,
    #[serde(default)]
    modelos: BTreeMap<String, Preco>,
}

/// De onde veio cada preço (para `cost pricing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origem {
    Embutido,
    Usuario,
    Projeto,
}

impl Origem {
    pub fn nome(self) -> &'static str {
        match self {
            Origem::Embutido => "embutido",
            Origem::Usuario => "usuario",
            Origem::Projeto => "projeto",
        }
    }
}

/// A tabela efetiva, depois de aplicar os overrides.
#[derive(Debug, Clone)]
pub struct Tabela {
    /// Versão do embutido, com `+usuario`/`+projeto` quando há override.
    pub versao: String,
    modelos: BTreeMap<String, (Preco, Origem)>,
}

impl Tabela {
    /// Só o embutido. O arquivo é parte do código e tem teste: se não parseia, é bug de build.
    pub fn embutida() -> Result<Tabela, String> {
        let a: Arquivo =
            toml::from_str(EMBUTIDO).map_err(|e| format!("pricing.toml embutido inválido: {e}"))?;
        let modelos = a.modelos.into_iter().map(|(k, v)| (k, (v, Origem::Embutido))).collect();
        Ok(Tabela { versao: a.versao, modelos })
    }

    /// Embutido + overrides (cada modelo do override substitui a entrada inteira).
    ///
    /// Override ilegível NÃO derruba a contabilidade: vira aviso e o resto segue. O erro de um
    /// arquivo do usuário não pode esconder o custo do projeto.
    pub fn carregar(usuario: Option<&Path>, projeto: Option<&Path>) -> (Tabela, Vec<String>) {
        let mut avisos = Vec::new();
        let mut t = match Tabela::embutida() {
            Ok(t) => t,
            Err(e) => {
                avisos.push(e);
                Tabela { versao: "indisponivel".into(), modelos: BTreeMap::new() }
            }
        };
        for (caminho, origem) in [(usuario, Origem::Usuario), (projeto, Origem::Projeto)] {
            let Some(p) = caminho else { continue };
            let txt = match std::fs::read_to_string(p) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    avisos.push(format!("pricing {}: não consegui ler ({e})", p.display()));
                    continue;
                }
            };
            match toml::from_str::<Arquivo>(&txt) {
                Ok(a) => {
                    for (k, v) in a.modelos {
                        t.modelos.insert(k, (v, origem));
                    }
                    t.versao = format!("{}+{}", t.versao, origem.nome());
                }
                Err(e) => avisos.push(format!("pricing {} inválido, ignorado: {e}", p.display())),
            }
        }
        (t, avisos)
    }

    /// Acha o preço do modelo: id exato, ou sem sufixo de data (`-20261001`) e de contexto
    /// (`[1m]`). Nada de casar por prefixo solto — `opus-5` não pode herdar o preço do `opus-5-5`.
    pub fn buscar(&self, modelo: &str) -> Option<(&str, &Preco, Origem)> {
        let base = modelo.split('[').next().unwrap_or(modelo);
        let sem_data = match base.rsplit_once('-') {
            Some((resto, sufixo))
                if sufixo.len() == 8 && sufixo.bytes().all(|b| b.is_ascii_digit()) =>
            {
                resto
            }
            _ => base,
        };
        for cand in [modelo, base, sem_data] {
            if let Some((k, (p, o))) = self.modelos.get_key_value(cand) {
                return Some((k.as_str(), p, *o));
            }
        }
        None
    }

    /// US$ da resposta; `None` se o modelo não tem preço.
    pub fn custo(&self, modelo: &str, t: &Tokens) -> Option<f64> {
        let (_, p, _) = self.buscar(modelo)?;
        let m = |n: u64, preco: f64| n as f64 * preco;
        let total = m(t.inp, p.input)
            + m(t.out, p.output)
            + m(t.cache_w5, p.cache_write_5m)
            + m(t.cache_w1h, p.cache_write_1h)
            + m(t.cache_r, p.cache_read);
        Some(total / 1_000_000.0)
    }

    /// Todos os modelos, em ordem alfabética.
    pub fn modelos(&self) -> impl Iterator<Item = (&str, &Preco, Origem)> {
        self.modelos.iter().map(|(k, (p, o))| (k.as_str(), p, *o))
    }

    /// Problemas de consistência de um preço (para `cost pricing --check`).
    pub fn problemas(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (k, p, _) in self.modelos() {
            let campos = [
                ("input", p.input),
                ("output", p.output),
                ("cache_write_5m", p.cache_write_5m),
                ("cache_write_1h", p.cache_write_1h),
                ("cache_read", p.cache_read),
            ];
            for (nome, val) in campos {
                if !val.is_finite() || val < 0.0 {
                    v.push(format!("{k}: {nome} inválido ({val})"));
                }
            }
            if p.output < p.input {
                v.push(format!("{k}: output menor que input (suspeito)"));
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tk(inp: u64, out: u64, w5: u64, w1h: u64, r: u64) -> Tokens {
        Tokens { inp, out, cache_w5: w5, cache_w1h: w1h, cache_r: r }
    }

    #[test]
    fn embutido_parseia_e_e_consistente() {
        let t = Tabela::embutida().unwrap();
        assert!(t.problemas().is_empty(), "{:?}", t.problemas());
        assert!(t.buscar("claude-opus-5-5").is_some());
        assert!(!t.versao.is_empty());
    }

    #[test]
    fn cache_read_e_write_tem_preco_proprio() {
        let t = Tabela::embutida().unwrap();
        // sonnet-5-5: in 2, out 10, w5 2.5, w1h 4, read 0.1 (US$/Mtok).
        let usd = t
            .custo("claude-sonnet-5-5", &tk(1_000_000, 1_000_000, 1_000_000, 1_000_000, 1_000_000));
        assert!((usd.unwrap() - (2.0 + 10.0 + 2.5 + 4.0 + 0.1)).abs() < 1e-9);
        // só leitura de cache custa 1/20 da entrada
        let r = t.custo("claude-sonnet-5-5", &tk(0, 0, 0, 0, 2_000_000)).unwrap();
        assert!((r - 0.2).abs() < 1e-9);
    }

    #[test]
    fn modelo_sem_preco_e_none() {
        let t = Tabela::embutida().unwrap();
        assert!(t.custo("claude-inventado-9", &tk(10, 10, 0, 0, 0)).is_none());
        // prefixo não herda preço de irmão
        assert!(t.buscar("claude-opus").is_none());
    }

    #[test]
    fn sufixos_de_data_e_contexto_resolvem() {
        let t = Tabela::embutida().unwrap();
        assert_eq!(t.buscar("claude-opus-5-5-20261001").unwrap().0, "claude-opus-5-5");
        assert_eq!(t.buscar("claude-opus-5-5[1m]").unwrap().0, "claude-opus-5-5");
    }

    #[test]
    fn override_do_projeto_vence_o_usuario_e_o_embutido() {
        let d = tempfile::tempdir().unwrap();
        let u = d.path().join("u.toml");
        let p = d.path().join("p.toml");
        let corpo = |i: f64| {
            format!(
                "[modelos.claude-opus-5-5]\ninput={i}\noutput=1\ncache_write_5m=1\n\
                 cache_write_1h=1\ncache_read=1\n"
            )
        };
        std::fs::write(&u, corpo(7.0)).unwrap();
        std::fs::write(&p, corpo(9.0)).unwrap();
        let (t, av) = Tabela::carregar(Some(&u), Some(&p));
        assert!(av.is_empty());
        let (_, preco, o) = t.buscar("claude-opus-5-5").unwrap();
        assert_eq!((preco.input, o), (9.0, Origem::Projeto));
        assert!(t.versao.ends_with("+usuario+projeto"));
        // modelo que o override não toca segue embutido
        assert_eq!(t.buscar("claude-sonnet-5-5").unwrap().2, Origem::Embutido);
    }

    #[test]
    fn override_corrompido_vira_aviso_sem_derrubar() {
        let d = tempfile::tempdir().unwrap();
        let u = d.path().join("u.toml");
        std::fs::write(&u, "isto = não é toml [").unwrap();
        let (t, av) = Tabela::carregar(Some(&u), Some(&d.path().join("ausente.toml")));
        assert_eq!(av.len(), 1);
        assert!(t.buscar("claude-opus-5-5").is_some());
    }
}
