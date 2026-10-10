//! Biblioteca do schematize — módulos compartilhados entre o CLI (`schematize`) e a
//! GUI (`schematize-gui`). O quê: expõe registry/skills/overdev/agent/autostart/settings,
//! além de i18n (multi-idioma), config, doctor, upgrade, news, status e links.

pub mod account;
pub mod agent;
pub mod appicon;
pub mod applink;
pub mod autostart;
pub mod binlink;
pub mod config;
pub mod debug;
pub mod debugreport;
pub mod deployerlink;
pub mod diagnostics;
pub mod disco;
pub mod doctor;
pub mod gestorboot;
/// Qual commit da GUI um release publica — o espelho do `lockpin` dela.
pub mod guipin;
pub mod i18n;
pub mod instalardica;
pub mod links;
pub mod market;
pub mod news;
pub mod notificacoes;
pub mod notifications;
pub mod overdevlink;
pub mod paths;
pub mod procedencia;
pub mod projects;
pub mod selfupdate;
pub mod skillslink;
pub mod status;
pub mod upgrade;
pub mod util;
pub mod versoes;
