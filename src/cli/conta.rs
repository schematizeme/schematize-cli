//! Subcomandos de CONTA e notificações: login por device flow, logout, whoami.

use schematize::{
    account,
    notifications, util,
};
use std::time::Duration;

/// `schematize notifications` — imprime as notificações agregadas, agrupadas por escopo.
pub(crate) fn notifications_cmd() {
    let all = notifications::collect();
    if all.is_empty() {
        println!("Sem notificações no momento.");
        return;
    }
    let global: Vec<_> = all
        .iter()
        .filter(|n| matches!(n.scope, notifications::NotifScope::Global))
        .collect();
    let personal: Vec<_> = all
        .iter()
        .filter(|n| matches!(n.scope, notifications::NotifScope::Personal))
        .collect();

    let print_group = |titulo: &str, ns: &[&notifications::Notif]| {
        if ns.is_empty() {
            return;
        }
        println!("{titulo} ({}):", ns.len());
        for n in ns {
            println!("  • [{}] {}", n.kind, n.title);
            if !n.body.trim().is_empty() {
                println!("    {}", n.body);
            }
            if let Some(a) = &n.action {
                println!("    → {a}");
            }
        }
    };
    print_group("Globais", &global);
    print_group("Pessoais", &personal);
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
