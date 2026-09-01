//! Biblioteca do schematize — módulos compartilhados entre o CLI (`schematize`) e a
//! GUI (`schematize-gui`). O quê: expõe registry/skills/overdev/agent/autostart/settings,
//! além de i18n (multi-idioma), config, doctor, upgrade, news, status e links.

pub mod account;
pub mod agent;
pub mod agentrun;
pub mod agents;
pub mod appicon;
pub mod archive;
pub mod autostart;
pub mod config;
pub mod database;
pub mod debug;
pub mod debugreport;
pub mod diagnostics;
pub mod disco;
pub mod doctor;
pub mod environments;
pub mod gitcontas;
pub mod githist;
pub mod guiactions;
/// Qual commit da GUI um release publica — o espelho do `lockpin` dela.
pub mod guipin;
pub mod i18n;
pub mod links;
pub mod market;
pub mod mcp;
pub mod news;
pub mod notificacoes;
pub mod notifications;
pub mod overdev;
pub mod overdevdb;
pub mod panel;
pub mod paths;
pub mod projects;
pub mod registry;
pub mod selfupdate;
pub mod settings;
pub mod skilledit;
pub mod skills;
pub mod skillsproj;
pub mod sshkeys;
pub mod status;
pub mod updaterboot;
pub mod upgrade;
pub mod usage;
pub mod util;
pub mod vps;
