//! Biblioteca do schematize — módulos compartilhados entre o CLI (`schematize`) e a
//! GUI (`schematize-gui`). O quê: expõe registry/skills/overdev/agent/autostart/settings,
//! além de i18n (multi-idioma), config, doctor, upgrade, news, status e links.

pub mod agent;
pub mod appicon;
pub mod autostart;
pub mod config;
pub mod debug;
pub mod environments;
pub mod doctor;
pub mod i18n;
pub mod links;
pub mod news;
pub mod overdev;
pub mod panel;
pub mod projects;
pub mod registry;
pub mod selfupdate;
pub mod settings;
pub mod skills;
pub mod status;
pub mod upgrade;
pub mod util;
