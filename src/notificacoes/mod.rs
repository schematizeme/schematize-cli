//! Fronteira de confiança e persistência das notificações.
//!
//! [`formato`] é a porta: o único caminho por onde dado de rede vira notificação, com
//! forma FECHADA e deny-by-default. [`cache`] é o que faz o badge e o painel lerem a
//! MESMA coisa — e o que preserva o histórico do que já foi resolvido.
//!
//! A coleta em si (quem fala com o blog, com a API, com o estado das skills) segue em
//! `crate::notifications`; aqui só mora o que precisa de garantia forte e teste.

pub mod cache;
pub mod formato;
