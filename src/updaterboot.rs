//! Garantia AUTOMÁTICA do gestor de atualizações (`schematize-updater`).
//!
//! O quê: verifica se o updater está instalado e, se faltar, baixa o binário da
//! plataforma — sem o usuário pedir. Onde: chamado no arranque da GUI (numa
//! thread), pelo `schematize doctor` e pelo agente.
//!
//! Por quê existe: o updater é quem sabe atualizar o app direito (build
//! incremental, cross-OS, sem sudo travando sem TTY). Se ele não estiver na
//! máquina, o caminho de update degrada pro fluxo interno — e é aí que "cliquei
//! em atualizar e não aconteceu nada" volta. Ele era instalado só pelo
//! `install.sh` (best-effort, silencioso se a rede falhasse) ou por um BOTÃO que
//! ninguém clica. Deixar isso na mão do usuário é o oposto do piso de "prever o
//! macaco": quem instala o app não tem de saber que existe um gestor separado.
//!
//! Cuidado com rede: quando o updater ESTÁ presente a checagem é só um `stat` —
//! zero rede. Quando falta, a tentativa é limitada por um carimbo em disco
//! ([`RETRY_EVERY`]), pra máquina offline não bater no GitHub a cada abertura.

use crate::selfupdate;
use crate::util;
use std::fs;
use std::path::PathBuf;

/// Intervalo mínimo entre tentativas de instalar o updater quando ele falta.
/// 6 h: pega o "abri de manhã, a rede voltou" sem virar poluição de rede.
const RETRY_EVERY: u64 = 6 * 60 * 60;

/// Carimbo (epoch da última tentativa) em `~/.claude/schematize/updater-boot.stamp`.
fn stamp_path() -> PathBuf {
    util::config_path()
        .parent()
        .map(|p| p.join("updater-boot.stamp"))
        .unwrap_or_else(|| PathBuf::from("updater-boot.stamp"))
}

/// Epoch da última tentativa (0 se nunca tentou / carimbo ilegível).
fn last_try() -> u64 {
    fs::read_to_string(stamp_path())
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Regrava o carimbo com o instante atual. Best-effort (falhar aqui só faz
/// tentar de novo na próxima — nunca quebra o arranque).
fn mark_try() {
    let p = stamp_path();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    let _ = fs::write(&p, util::now_unix().to_string());
}

/// O updater já está na máquina? Checagem LOCAL (PATH + `~/.cargo/bin` +
/// `~/.local/bin`), sem tocar na rede — barata o bastante pra rodar no arranque.
pub fn present() -> bool {
    selfupdate::updater_bin().is_some()
}

/// Resultado de uma tentativa de garantir o updater.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Já estava instalado — nada foi feito (e nenhuma rede foi usada).
    JaTinha,
    /// Faltava e foi instalado agora, neste caminho.
    Instalado(PathBuf),
    /// Faltava, mas a última tentativa foi recente demais — não insistimos ainda.
    Adiado,
    /// Faltava, tentamos, e falhou (rede/plataforma sem binário). Mensagem crua.
    Falhou(String),
}

/// Garante o updater instalado. BLOQUEIA (baixa da rede quando falta) — chame de
/// uma thread, nunca do event loop da GUI.
///
/// Nunca propaga erro pro chamador como `Err`: um app que não conseguiu instalar
/// o gestor de atualizações continua funcionando: o [`Outcome`] diz o que houve
/// pra quem quiser mostrar/logar.
pub fn ensure_now() -> Outcome {
    if present() {
        return Outcome::JaTinha;
    }
    if util::now_unix().saturating_sub(last_try()) < RETRY_EVERY {
        return Outcome::Adiado;
    }
    mark_try();
    match selfupdate::ensure_updater() {
        Ok(p) => Outcome::Instalado(p),
        Err(e) => Outcome::Falhou(e),
    }
}

/// Dispara [`ensure_now`] numa thread e devolve na hora. Pra quem só quer o
/// efeito colateral (agente, `doctor`) sem esperar a rede.
pub fn ensure_in_background() {
    if present() {
        return; // caminho comum: nem cria thread
    }
    std::thread::spawn(|| {
        let _ = ensure_now();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Com o updater presente, `ensure_now` sai sem tocar em rede nem carimbo —
    /// é o caminho que roda em toda abertura do app.
    #[test]
    fn presente_nao_faz_nada() {
        if present() {
            assert_eq!(ensure_now(), Outcome::JaTinha);
        }
    }

    /// O carimbo é o que segura a tentativa numa máquina offline: gravado agora,
    /// a janela de retry ainda não passou.
    #[test]
    fn carimbo_segura_a_retentativa() {
        mark_try();
        assert!(util::now_unix().saturating_sub(last_try()) < RETRY_EVERY);
    }
}
