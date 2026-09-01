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
    // `env` traz SO e hardware como campos FILTRÁVEIS. Antes só existiam dentro do blob
    // `report`, o que permitia ler um relato mas não cruzar vários ("quais travaram em
    // Wayland com GPU AMD?") sem reparsear centenas de KB. O blob segue junto.
    let body = serde_json::json!({
        "app": "cli",
        "version": app_ver,
        "os": os,
        "env": debugreport::ambiente(),
        "machine_id": machine_id(),
        "report": report,
    });
    let tmp = std::env::temp_dir().join(format!("schematize-diag-{}.json", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(&body).unwrap_or_default())
        .map_err(|e| format!("não consegui preparar o envio: {e}"))?;

    let url = format!("{}/diagnostics", account::api_base());
    let auth = format!("Authorization: Bearer {token}");
    let data = format!("@{}", tmp.display());
    // `%{http_code}` no fim do corpo: `curl -sS` SAI 0 num 404/500 (a transação HTTP
    // aconteceu), então o exit code NÃO diz se o servidor aceitou. Sem isto, um POST pra
    // uma rota inexistente devolvia Ok("") e o comando imprimia "✓ enviado" — sucesso
    // mentiroso, o pior tipo de bug num coletor: o usuário acha que reportou e não há
    // nada do outro lado. Ver `resposta_do_envio`.
    let out = util::run(
        "curl",
        &[
            "-sS",
            "-m",
            "30",
            "-X",
            "POST",
            "-H",
            "User-Agent: schematize-cli",
            "-H",
            "Content-Type: application/json",
            "-H",
            &auth,
            "--data-binary",
            &data,
            "-w",
            "\n%{http_code}",
            &url,
        ],
    );
    let _ = std::fs::remove_file(&tmp);

    let bruto = out.map_err(|e| format!("falha no envio (rede): {e}"))?;
    let corpo = resposta_do_envio(&bruto, &url)?;
    let id = id_do_recibo(&corpo);
    registrar_envio(&id, app_ver);
    match &id {
        Some(id) => println!(
            "✓ enviado. id do relatório: {id}\n  (guardado em ~/.schematize/diagnostics-sent.log — \
             cite esse id ao pedir análise)"
        ),
        None => println!(
            "✓ enviado, mas o servidor não devolveu `id` no corpo: {}\n  \
             Sem id não dá pra referenciar o relatório depois — reporte isto.",
            corpo.trim()
        ),
    }
    Ok(())
}

/// Extrai o `id` do recibo `202 {status, id}`.
///
/// O quê: lê o campo `id` do JSON de resposta. Onde: [`send`], logo após o 2xx.
/// Por que existe: o id é a ÚNICA forma de referenciar um relatório depois (não há rota de
/// leitura ainda; quando houver, é por ele que se busca). Imprimir e esquecer torna o envio
/// irrastreável — o usuário sabe que mandou e não sabe o quê.
/// **Entrada:** corpo da resposta. **Saída:** o id, ou `None` se ausente/ilegível (o envio
/// já aconteceu; não é erro). **Efeitos:** nenhum.
fn id_do_recibo(corpo: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(corpo).ok()?;
    v.get("id")?.as_str().map(|s| s.to_string())
}

/// Anexa o envio a `~/.schematize/diagnostics-sent.log` (best-effort).
///
/// O quê: uma linha por envio — epoch, versão e id. Onde: [`send`].
/// Por que: sem registro local, "eu já mandei isso?" não tem resposta, e o id do recibo se
/// perde no scrollback do terminal. **Efeitos:** escreve no HOME; erro é ignorado de
/// propósito — falhar o registro não pode invalidar um envio que já ocorreu.
fn registrar_envio(id: &Option<String>, versao: &str) {
    let path = home_dir().join("diagnostics-sent.log");
    let _ = std::fs::create_dir_all(home_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(
            f,
            "{} v{versao} id={}",
            util::now_unix(),
            id.as_deref().unwrap_or("(sem id no recibo)")
        );
    }
}

/// Decide se o POST foi ACEITO, a partir da saída do `curl -w '\n%{http_code}'`.
///
/// O quê: separa o código HTTP (última linha) do corpo e só considera sucesso em `2xx`.
/// Onde: [`send`], logo após o curl — e é a peça PURA que o teste exercita sem rede.
///
/// Por que existe: `curl -sS` sai **0** em 404/500, porque do ponto de vista dele a
/// transação foi bem-sucedida. Confiar no exit code fazia o comando anunciar "✓ enviado"
/// pra uma rota que não existe. A mensagem de erro é acionável (§48: diz o que houve e o
/// que fazer), e distingue "rota ausente" de "recusado" — são problemas diferentes.
///
/// **Entrada:** `bruto` — stdout do curl (corpo + `\n` + código); `url` — pra mensagem.
/// **Saída:** `Ok(corpo)` em 2xx; `Err(mensagem acionável)` em qualquer outro caso.
/// **Efeitos:** nenhum.
fn resposta_do_envio(bruto: &str, url: &str) -> Result<String, String> {
    let (corpo, code) = match bruto.rsplit_once('\n') {
        Some((c, code)) => (c, code.trim()),
        // Sem `\n`: ou o corpo é vazio e sobrou só o código, ou o curl não escreveu nada.
        None => ("", bruto.trim()),
    };
    let status: u16 = code.parse().map_err(|_| {
        format!("resposta ilegível do servidor (esperava um código HTTP, veio {code:?}). Nada foi confirmado.")
    })?;
    match status {
        200..=299 => Ok(corpo.to_string()),
        404 => Err(format!(
            "o servidor não tem a rota de diagnóstico ({url} respondeu 404).              NADA foi armazenado — não adianta reenviar. Guarde o relatório local com              `schematize debug --collect` e reporte que a rota está ausente."
        )),
        401 | 403 => Err(format!(
            "o servidor recusou a sessão (HTTP {status}). Refaça o login com              `schematize account login` e tente de novo. Nada foi enviado."
        )),
        _ => Err(format!(
            "o servidor respondeu HTTP {status} — o relatório NÃO foi armazenado.              Corpo: {}",
            if corpo.trim().is_empty() { "(vazio)" } else { corpo.trim() }
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O QUE: 404 é FALHA, não sucesso — o caso real que o `/diagnostics` inexistente
    /// produzia (corpo vazio + código na última linha) e que o comando anunciava como
    /// "✓ enviado" porque só olhava o exit code do curl.
    #[test]
    fn quatrocentos_e_quatro_nao_e_envio() {
        let r = resposta_do_envio("\n404", "https://api.x/diagnostics");
        let e = r.expect_err("404 tem que ser erro");
        assert!(e.contains("404"), "a mensagem precisa dizer o código: {e}");
        assert!(e.contains("NADA foi armazenado"), "precisa ser explícito: {e}");
    }

    /// O QUE: 2xx passa e devolve o corpo sem o código pendurado.
    #[test]
    fn dois_centos_e_envio_com_corpo_limpo() {
        let corpo = resposta_do_envio("{\"id\":\"abc\"}\n201", "https://api.x/diagnostics")
            .expect("201 é sucesso");
        assert_eq!(corpo, "{\"id\":\"abc\"}", "o código HTTP não pode vazar no corpo");
    }

    /// O QUE: sessão recusada tem mensagem PRÓPRIA — reenviar não resolve 401, refazer
    /// o login resolve; dizer "falhou" genérico faria o usuário repetir à toa.
    #[test]
    fn sessao_recusada_ensina_o_caminho() {
        let e = resposta_do_envio("\n401", "https://api.x/diagnostics").expect_err("401 é erro");
        assert!(e.contains("login"), "a mensagem tem que ser acionável: {e}");
    }

    /// O QUE: o id do recibo é extraído do 202 — é a única alça pra referenciar o
    /// relatório depois (não existe rota de leitura ainda; quando existir, busca-se por ele).
    #[test]
    fn extrai_o_id_do_recibo() {
        assert_eq!(
            id_do_recibo(r#"{"status":"received","id":"01J8Z…"}"#).as_deref(),
            Some("01J8Z…")
        );
        // Recibo sem id não é erro (o envio ocorreu) — mas tem que virar None, pra o
        // comando avisar em vez de fingir que guardou algo.
        assert_eq!(id_do_recibo(r#"{"status":"received"}"#), None);
        assert_eq!(id_do_recibo("não json"), None);
    }

    /// O QUE: 500 também é falha (o relatório não foi guardado), com o corpo preservado
    /// pra triagem.
    #[test]
    fn erro_do_servidor_nao_e_envio() {
        let e =
            resposta_do_envio("boom\n500", "https://api.x/diagnostics").expect_err("500 é erro");
        assert!(e.contains("500") && e.contains("boom"), "{e}");
    }
}
