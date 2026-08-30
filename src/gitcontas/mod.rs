//! GIT/GITHUB — contas, repositórios e histórico.
//!
//! O quê: quais contas existem na máquina, qual delas vale em cada repositório, os
//! repositórios do serviço, e o histórico do que já foi commitado/enviado. Onde:
//! `schematize git` no CLI e a tela Git da GUI.
//!
//! Divisão: [`contas`] guarda o cadastro (sem segredo), [`deteccao`] descobre as que já
//! existem na máquina (gh/git config/ssh/repos), [`aplicar`] escreve a
//! identidade no repo, [`repos`] fala com o serviço, e o histórico de commits reusa o
//! `githist` que já existia (não duplicamos leitura de git log).

pub mod aplicar;
pub mod deteccao;
pub mod contas;
pub mod repos;
