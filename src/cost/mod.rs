//! Contabilidade de custo (tokens + US$) por projeto — o subsistema `schematize cost`.
//!
//! **O quê:** lê os transcripts do Claude Code, deduplica as respostas (o streaming repete a
//! mesma resposta em várias linhas), precifica com o `pricing.toml` e grava um ledger
//! append-only no archive do projeto, de onde saem `custos.json` (máquina) e `custos.md`
//! (humano). Contrato: `schematize_app_archive/contratos/custos-v1.md`.
//!
//! **Onde:** o CLI (`schematize cost …`) e a GUI (que só LÊ `custos.json`, não reimplementa o
//! parser). Tudo vive em `<projeto>_archive/custos/`, versionado em git.
//!
//! **Divergências do contrato (registradas):**
//! - A chave de dedupe é só `message.id`, sem `requestId`: o id da mensagem já é único por
//!   resposta, e compor com `requestId` racha a chave quando as linhas de streaming não o
//!   trazem todas — o que contaria a resposta duas vezes.
//! - O ledger ganha `chave` (`message.id`; sem ela não há idempotência no arquivo) e
//!   `cache_w1h` (parte de `cache_w` cobrada como escrita de 1h; permite reprecificar).
//! - Se uma resposta reaparece em outro `sync` com mais `output_tokens`, anexa-se uma linha
//!   nova com a mesma `chave`; na leitura vale a ÚLTIMA linha de cada chave. O arquivo
//!   continua append-only.

pub mod agregar;
pub mod ledger;
pub mod parser;
pub mod preco;
pub mod projeto;
pub mod relatorio;
pub mod sync;
pub mod tasks;
pub mod tempo;

/// Aviso que acompanha QUALQUER número de custo: é estimativa, não fatura.
pub const AVISO_ESTIMATIVA: &str =
    "estimativa API-equivalente; assinatura/plano pode cobrar diferente; confira o /usage";

/// Task de quem não tem tag nem item ativo do overdev.
pub const SEM_TASK: &str = "sem-task";

/// Tokens de UMA resposta, já separados nas cinco rubricas que têm preço próprio.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub inp: u64,
    pub out: u64,
    /// Escrita de cache de 5 min.
    pub cache_w5: u64,
    /// Escrita de cache de 1 h.
    pub cache_w1h: u64,
    pub cache_r: u64,
}

impl Tokens {
    /// Escrita de cache total (5m + 1h), como o ledger a guarda.
    pub fn cache_w(&self) -> u64 {
        self.cache_w5 + self.cache_w1h
    }
}
