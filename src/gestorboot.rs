//! Garantia AUTOMÁTICA do gestor de atualizações (`schematize-market`).
//!
//! O quê: verifica se o gestor está instalado e, se faltar, baixa o binário da
//! plataforma — sem o usuário pedir. Onde: chamado no arranque da GUI (numa
//! thread), pelo `schematize doctor` e pelo agente.
//!
//! Por quê existe: o gestor é quem sabe atualizar o app direito (build
//! incremental, cross-OS, sem sudo travando sem TTY). Se ele não estiver na
//! máquina, o caminho de update degrada pro fluxo interno — e é aí que "cliquei
//! em atualizar e não aconteceu nada" volta. Ele era instalado só pelo
//! `install.sh` (best-effort, silencioso se a rede falhasse) ou por um BOTÃO que
//! ninguém clica. Deixar isso na mão do usuário é o oposto do piso de "prever o
//! macaco": quem instala o app não tem de saber que existe um gestor separado.
//!
//! ADR-0013: até então o gestor era o `schematize-updater`, absorvido pelo
//! `schematize-market`. Este módulo aponta para o sucessor — e o carimbo em disco
//! do antecessor é LIDO, não descartado (ver [`stamp_path`]).
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

/// Carimbo (epoch da última tentativa) em `~/.claude/schematize/gestor-boot.stamp`.
fn stamp_path() -> PathBuf {
    carimbo("gestor-boot.stamp")
}

/// O carimbo com o nome ANTIGO, de antes do ADR-0013.
///
/// **Por que continua sendo lido:** o carimbo é o que segura a tentativa de download numa
/// máquina OFFLINE — sem ele, o app volta a bater no GitHub a cada abertura. Numa máquina
/// que já tinha o app, o arquivo existe com o nome velho; ignorá-lo zeraria a janela de
/// retentativa de toda a base instalada, exatamente na atualização que troca o gestor.
fn stamp_path_legado() -> PathBuf {
    carimbo("updater-boot.stamp")
}

/// Monta o caminho de um carimbo no diretório de config.
fn carimbo(nome: &'static str) -> PathBuf {
    util::config_path().parent().map(|p| p.join(nome)).unwrap_or_else(|| PathBuf::from(nome))
}

/// Epoch da última tentativa (0 se nunca tentou / carimbo ilegível).
///
/// Lê o carimbo novo e, se ele não existir, o do nome antigo — a tentativa mais RECENTE dos
/// dois é a que vale.
fn last_try() -> u64 {
    let ler = |p: PathBuf| -> u64 {
        fs::read_to_string(p).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0)
    };
    ler(stamp_path()).max(ler(stamp_path_legado()))
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

/// O gestor já está na máquina? Checagem LOCAL (PATH + `~/.cargo/bin` +
/// `~/.local/bin`), sem tocar na rede — barata o bastante pra rodar no arranque.
pub fn present() -> bool {
    selfupdate::gestor_bin().is_some()
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

/// A janela de retentativa já passou? Puro — o carimbo entra por parâmetro.
fn pode_tentar(agora: u64, ultima: u64) -> bool {
    agora.saturating_sub(ultima) >= RETRY_EVERY
}

/// Garante o gestor instalado. BLOQUEIA (baixa da rede quando falta) — chame de
/// uma thread, nunca do event loop da GUI.
///
/// Automático: respeita a janela de [`RETRY_EVERY`], pra máquina offline não bater
/// no GitHub a cada abertura. Quando o usuário PEDIU (o `doctor`), use
/// [`ensure_now_forcado`] — aí a janela não se aplica.
///
/// Nunca propaga erro como `Err`: um app que não conseguiu instalar o gestor de
/// atualizações continua funcionando; o [`Outcome`] diz o que houve pra quem
/// quiser mostrar/logar.
pub fn ensure_now() -> Outcome {
    if present() {
        return Outcome::JaTinha;
    }
    if !pode_tentar(util::now_unix(), last_try()) {
        return Outcome::Adiado;
    }
    ensure_now_forcado()
}

/// Igual a [`ensure_now`], mas IGNORA a janela de retentativa.
///
/// É o que o `schematize doctor` chama: ali o usuário pediu explicitamente pra
/// consertar a máquina, e responder "tento de novo mais tarde" a um pedido direto
/// é o tipo de resposta que faz a ferramenta parecer quebrada.
pub fn ensure_now_forcado() -> Outcome {
    if present() {
        return Outcome::JaTinha;
    }
    mark_try();
    match selfupdate::ensure_gestor() {
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

    /// Com o gestor presente, `ensure_now` sai sem tocar em rede nem carimbo —
    /// é o caminho que roda em toda abertura do app.
    #[test]
    fn presente_nao_faz_nada() {
        if present() {
            assert_eq!(ensure_now(), Outcome::JaTinha);
        }
    }

    /// **ADR-0013:** o carimbo com o nome ANTIGO continua valendo. Sem isto, a troca de
    /// gestor zeraria a janela de retentativa de toda a base instalada, e toda máquina
    /// offline voltaria a bater no GitHub a cada abertura do app.
    #[test]
    fn o_carimbo_do_nome_antigo_continua_sendo_lido() {
        assert!(stamp_path().ends_with("gestor-boot.stamp"));
        assert!(stamp_path_legado().ends_with("updater-boot.stamp"));
        assert_eq!(
            stamp_path().parent(),
            stamp_path_legado().parent(),
            "os dois carimbos moram no mesmo diretório"
        );
        assert_ne!(stamp_path(), stamp_path_legado());
    }

    /// A janela de retentativa é o que segura a tentativa numa máquina offline.
    /// Testa a REGRA (pura) — não o carimbo em disco: um teste que escreve o
    /// carimbo de verdade envenena a janela do usuário que roda `cargo test`
    /// (foi o que aconteceu: o `doctor` respondeu "tento de novo mais tarde"
    /// porque a suíte tinha acabado de carimbar).
    #[test]
    fn janela_de_retentativa() {
        let agora = 1_000_000u64;
        assert!(!pode_tentar(agora, agora), "acabou de tentar: não insiste");
        assert!(!pode_tentar(agora, agora - RETRY_EVERY + 1), "ainda dentro da janela");
        assert!(pode_tentar(agora, agora - RETRY_EVERY), "janela fechada: pode tentar");
        assert!(pode_tentar(agora, 0), "nunca tentou: pode tentar");
    }
}
