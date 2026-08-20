//! diagnostics — envia (OPT-IN) o relatório de debug REDIGIDO pro servidor, pra triagem.
//!
//! O quê: `send()` coleta o MESMO relatório do `schematize debug --collect` (já passado pelo
//! `scrub()` — sem tokens/segredos/PII), confirma com o usuário e faz `POST /diagnostics` na API
//! autenticado com a sessão. Onde: subcomando `schematize diagnostics send`. Postura de PRIVACIDADE
//! (piso da casa): **nada é enviado por padrão** — só neste comando explícito, e só depois do "sim".
//! O corpo vai por arquivo temporário (report pode ter centenas de KB) e é limitado no cliente.

use crate::{account, debugreport, util};
use std::io::Write;
use std::path::PathBuf;

/// Teto do relatório no cliente (o servidor também recusa acima disso). 256 KB.
const REPORT_CAP: usize = 256 * 1024;

/// Dir operacional do usuário (`~/.schematize`).
fn home_dir() -> PathBuf {
    util::home_app_dir()
}

/// ID ANÔNIMO e estável da máquina — NÃO é PII (16 bytes aleatórios em hex, gerados uma vez em
/// `~/.schematize/machine-id`). Serve só pra correlacionar relatórios do mesmo aparelho sem
/// identificar a pessoa. Se não der pra ler urandom, cai num id efêmero.
fn machine_id() -> String {
    let path = home_dir().join("machine-id");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let t = existing.trim();
        if t.len() >= 8 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return t.to_string();
        }
    }
    let id = rand_hex_16();
    let _ = std::fs::create_dir_all(home_dir());
    let _ = std::fs::write(&path, &id);
    id
}

/// 16 bytes de /dev/urandom em hex (sem crate de random). Fallback: nome do host + pid.
fn rand_hex_16() -> String {
    if let Ok(bytes) = std::fs::read("/dev/urandom") {
        if bytes.len() >= 16 {
            return bytes[..16].iter().map(|b| format!("{b:02x}")).collect();
        }
    }
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "host".into());
    format!("{}-{}", host, std::process::id())
}

/// SO normalizado pro contrato do servidor (`linux`|`macos`|`windows`).
fn os_tag() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

/// Pergunta sim/não no terminal (default NÃO). Usado pra confirmar o envio (opt-in).
fn confirm(prompt: &str) -> bool {
    print!("{prompt} ");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    if std::io::stdin().read_line(&mut s).is_err() {
        return false;
    }
    matches!(s.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes")
}

/// Envia o relatório de diagnóstico (redigido) pro servidor. `yes` pula a confirmação.
pub fn send(yes: bool) -> Result<(), String> {
    if !account::is_logged_in() {
        return Err("faça login primeiro: `schematize account login`.".into());
    }
    let token = account::access_token().ok_or("sessão inválida — refaça o login.")?;

    // MESMO relatório do `debug --collect` — já REDIGIDO (scrub aplicado no fim do collect).
    let mut report = debugreport::collect(false);
    if report.len() > REPORT_CAP {
        report.truncate(REPORT_CAP);
        report.push_str("\n…(truncado no cliente)\n");
    }

    let app_ver = crate::upgrade::app_version();
    let os = os_tag();
    println!(
        "Vai ENVIAR um relatório de diagnóstico (JÁ REDIGIDO — sem tokens/segredos) pro servidor:\n\
         \x20 app: cli · versão: {app_ver} · SO: {os} · tamanho: {} KB\n\
         \x20 destino: {}/diagnostics\n\
         Veja o conteúdo exato com `schematize debug --collect --stdout`.",
        report.len() / 1024,
        account::api_base()
    );
    if !yes && !confirm("Enviar? [s/N]") {
        return Err("cancelado — nada foi enviado.".into());
    }

    // Corpo JSON via arquivo temporário (report pode ser grande demais pra caber num argv).
    let body = serde_json::json!({
        "app": "cli",
        "version": app_ver,
        "os": os,
        "machine_id": machine_id(),
        "report": report,
    });
    let tmp = std::env::temp_dir().join(format!("schematize-diag-{}.json", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(&body).unwrap_or_default())
        .map_err(|e| format!("não consegui preparar o envio: {e}"))?;

    let url = format!("{}/diagnostics", account::api_base());
    let auth = format!("Authorization: Bearer {token}");
    let data = format!("@{}", tmp.display());
    let out = util::run(
        "curl",
        &[
            "-sS", "-m", "30", "-X", "POST",
            "-H", "User-Agent: schematize-cli",
            "-H", "Content-Type: application/json",
            "-H", &auth,
            "--data-binary", &data,
            &url,
        ],
    );
    let _ = std::fs::remove_file(&tmp);

    match out {
        Ok(resp) => {
            println!("✓ enviado. resposta do servidor: {}", resp.trim());
            Ok(())
        }
        Err(e) => Err(format!("falha no envio: {e}")),
    }
}
