//! Subcomandos de CONTA e notificações: login por device flow, logout, whoami.

use schematize::{
    account,
    notifications, util,
};
use std::time::Duration;

/// `schematize notifications` — mostra o CACHE local (inclusive o histórico).
///
/// Lê do cache e sincroniza com a rede DEPOIS, não antes: assim o comando responde
/// instantaneamente e funciona offline. Era o contrário — ia à rede e, se ela falhasse,
/// dizia "sem notificações", que é diferente de "não consegui buscar".
pub(crate) fn notifications_cmd(sync: bool, historico: bool) {
    use schematize::notificacoes::cache::Estado;
    if sync {
        println!("sincronizando…");
        notifications::sincronizar();
    }
    let all = notifications::listar();
    let visiveis: Vec<_> = all
        .iter()
        .filter(|r| historico || r.estado != Estado::Concluida)
        .collect();
    if visiveis.is_empty() {
        if all.is_empty() {
            println!("sem notificações no cache — rode `schematize notifications --sync`.");
        } else {
            println!("nada pendente ({} no histórico; veja com --historico).", all.len());
        }
        return;
    }

    let grupo = |titulo: &str, escopo: &str| {
        let ns: Vec<_> = visiveis.iter().filter(|r| r.escopo == escopo).collect();
        if ns.is_empty() {
            return;
        }
        println!("\n\x1b[1m{titulo}\x1b[0m ({}):", ns.len());
        for r in ns {
            // O marcador diz o ESTADO, que é o que o histórico existe pra mostrar.
            let m = match r.estado {
                Estado::Nova => "\x1b[33m●\x1b[0m",
                Estado::Lida => "\x1b[2m○\x1b[0m",
                Estado::Concluida => "\x1b[32m✓\x1b[0m",
            };
            println!("  {m} [{}] {}", r.kind, r.titulo);
            if !r.corpo.trim().is_empty() {
                println!("      {}", r.corpo);
            }
            if !r.acao.is_empty() {
                println!("      → {}", r.acao);
            }
            println!("      \x1b[2mid {}\x1b[0m", r.id);
        }
    };
    grupo("Globais", "global");
    grupo("Pessoais", "personal");

    let novas = all.iter().filter(|r| r.estado == Estado::Nova).count();
    if novas > 0 {
        println!("\n{novas} não lida(s). `schematize notifications --lidas` marca todas como vistas.");
    }
    println!("concluir uma:  schematize notifications --concluir <id>");
}

/// Marca todas as não lidas como vistas. Não apaga nada.
pub(crate) fn notifications_lidas() {
    let n = notifications::marcar_lidas();
    println!("{n} notificação(ões) marcada(s) como lida(s). Nada foi apagado.");
}

/// Marca uma como concluída — vai pro histórico, não some.
pub(crate) fn notifications_concluir(id: &str) -> Result<(), String> {
    if notifications::concluir(id) {
        println!("concluída. Segue no histórico (`schematize notifications --historico`).");
        Ok(())
    } else {
        Err(format!("não achei a notificação '{id}'"))
    }
}

/// `schematize login` — autentica via OAuth device flow: inicia o fluxo, mostra o
/// `user_code` + a URL de verificação, e faz o polling respeitando `interval`/`slow_down`
/// até autorizar (Ok), negar (Denied) ou expirar. Salva a sessão em `~/.schematize/auth.json`.
pub(crate) fn login_cmd() -> Result<(), String> {
    if let Some(sub) = account::account_sub() {
        println!("Você já está logado como {sub}. (Para trocar de conta: `schematize logout`.)");
        return Ok(());
    }
    let dl = account::device_start()?;
    println!("Para entrar, abra no navegador:");
    println!("  {}", dl.verification_uri);
    println!("E informe o código: {}", dl.user_code);
    if dl.verification_uri_complete != dl.verification_uri {
        println!("\n(ou abra direto, já com o código: {})", dl.verification_uri_complete);
    }
    // Best-effort: já abre o navegador na URL completa (não bloqueia).
    util::open_url(&dl.verification_uri_complete);
    println!("\nAguardando você autorizar no navegador...");

    let mut interval = dl.interval.max(1);
    let deadline = util::now_unix() + dl.expires_in;
    loop {
        if util::now_unix() >= deadline {
            return Err("o código expirou — rode `schematize login` de novo.".to_string());
        }
        std::thread::sleep(Duration::from_secs(interval));
        match account::device_poll_once(&dl.device_code) {
            Ok(account::PollResult::Pending) => continue,
            Ok(account::PollResult::SlowDown) => {
                interval += 5; // servidor pediu pra desacelerar.
                continue;
            }
            Ok(account::PollResult::Denied) => {
                return Err("autorização negada. Nada foi salvo.".to_string());
            }
            Ok(account::PollResult::Expired) => {
                return Err("o código expirou — rode `schematize login` de novo.".to_string());
            }
            Ok(account::PollResult::Ok(tokens)) => {
                account::save_tokens(&tokens)?;
                println!("\n✓ Login efetuado! Você está logado como {}.", tokens.sub);
                return Ok(());
            }
            // Falha de rede numa tentativa: não aborta — segue tentando até o deadline.
            Err(e) => {
                eprintln!("(aviso de rede, tentando de novo: {e})");
                continue;
            }
        }
    }
}

/// `schematize logout` — apaga a sessão local.
pub(crate) fn logout_cmd() {
    if account::is_logged_in() {
        account::logout();
        println!("Sessão encerrada. Você não está mais logado.");
    } else {
        println!("Você já não estava logado.");
    }
}

/// `schematize whoami` — mostra o subject da conta logada (ou avisa que não há sessão).
pub(crate) fn whoami_cmd() {
    match account::account_sub() {
        Some(sub) => println!("Logado como: {sub}"),
        None => println!("Você não está logado. Rode `schematize login`."),
    }
}
