//! Conta do usuário — LOGIN na plataforma via OAuth 2.0 Device Authorization Grant.
//! O quê: fala com o IdP da casa (`schematize_auth_rs`) pra autenticar o usuário sem
//! abrir servidor local: pede um `user_code`, o usuário confirma no navegador, e a CLI
//! faz polling do `/token` até virar sessão. Persiste os tokens em `~/.schematize/auth.json`
//! (modo 600) e faz refresh transparente quando o access expira. Onde: consumido pelo CLI
//! (`login`/`logout`/`whoami`), pela GUI (login na janela) e pelas notificações do servidor.
//! Postura: TUDO é resiliente — falha de rede vira Err/None, nunca panica.
//!
//! Rede: segue o padrão da casa (curl por `util::run`, sem crate de HTTP). As funções de
//! parsing (`parse_device_resp`, `parse_token_at`) são PURAS — testáveis sem rede.

use crate::util;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ------------------------------------------------------------------------------------------------
// Endpoints — defaults com OVERRIDE por env.
// ------------------------------------------------------------------------------------------------

/// Base do IdP (device flow + token/refresh). Default `https://auth.schematize.org`,
/// sobrescrevível por `SCHEMATIZE_AUTH_URL`.
///
/// ATENÇÃO: o default é um palpite pelo domínio institucional (.org) — CONFIRME o host real
/// do `schematize_auth_rs` em produção (hoje ele roda local em `:8787`). Ajuste o default
/// aqui, ou exporte `SCHEMATIZE_AUTH_URL` (ex.: `http://127.0.0.1:8787`) enquanto não fecha.
const AUTH_BASE_DEFAULT: &str = "https://auth.schematize.org";

/// Base da API do marketplace (notificações etc.). Default `https://api.schematize.org`,
/// sobrescrevível por `SCHEMATIZE_API_URL`.
///
/// ATENÇÃO: mesmo caso do AUTH_BASE — o default (.org) é palpite; a API hoje roda local em
/// `:8080` (`schematizeskills_api_rs`). CONFIRME o host real ou use `SCHEMATIZE_API_URL`.
const API_BASE_DEFAULT: &str = "https://api.schematize.org";

/// `client_id` público do device flow (o IdP conhece `schematize`).
const CLIENT_ID: &str = "schematize";

/// Base do IdP em uso (env `SCHEMATIZE_AUTH_URL` tem prioridade sobre o default).
pub fn auth_base() -> String {
    env_or("SCHEMATIZE_AUTH_URL", AUTH_BASE_DEFAULT)
}

/// Base da API do marketplace (env `SCHEMATIZE_API_URL` tem prioridade sobre o default).
pub fn api_base() -> String {
    env_or("SCHEMATIZE_API_URL", API_BASE_DEFAULT)
}

/// Lê uma env não-vazia, senão devolve o default (aparando `/` final pra compor URLs limpas).
fn env_or(key: &str, default: &str) -> String {
    let raw = std::env::var(key).ok().filter(|v| !v.trim().is_empty());
    raw.unwrap_or_else(|| default.to_string()).trim_end_matches('/').to_string()
}

// ------------------------------------------------------------------------------------------------
// Tipos públicos.
// ------------------------------------------------------------------------------------------------

/// Resultado do início do device flow: o que a UI mostra ao usuário + o `device_code` do polling.
pub struct DeviceLogin {
    /// Código curto que o usuário digita no navegador (ex.: `WDJB-MJHT`).
    pub user_code: String,
    /// URL de verificação que o usuário abre (ex.: `https://auth.../device`).
    pub verification_uri: String,
    /// URL já com o código embutido (abre direto, sem digitar). Cai no `verification_uri` se ausente.
    pub verification_uri_complete: String,
    /// Segredo do polling (NÃO mostrar ao usuário) — vai no `/token`.
    pub device_code: String,
    /// Intervalo (s) mínimo entre polls, ditado pelo servidor.
    pub interval: u64,
    /// Validade (s) do `device_code` — depois disso, recomeçar.
    pub expires_in: u64,
}

/// Sessão persistida: os tokens + quem é o usuário. Serializada em `~/.schematize/auth.json`.
#[derive(Serialize, Deserialize, Clone)]
pub struct Tokens {
    /// Bearer de acesso (curto) usado nas chamadas à API.
    pub access_token: String,
    /// Token de refresh (rotacionado a cada uso) pra renovar o access sem novo login.
    pub refresh_token: String,
    /// Epoch (s) em que o `access_token` expira (= now + expires_in no momento da emissão).
    pub expires_at: u64,
    /// Subject — identificador estável do usuário (identidade ≠ email).
    pub sub: String,
}

/// Desfecho de UMA tentativa de polling no `/token`.
pub enum PollResult {
    /// Ainda não autorizado — continuar o polling no mesmo intervalo.
    Pending,
    /// Servidor pediu pra desacelerar — somar 5s ao intervalo e continuar.
    SlowDown,
    /// Usuário negou (ou grant inválido) — PARAR o polling.
    Denied,
    /// `device_code` expirou — PARAR (recomeçar o fluxo).
    Expired,
    /// Autorizado — sessão emitida.
    Ok(Tokens),
}

// ------------------------------------------------------------------------------------------------
// Persistência da sessão (`~/.schematize/auth.json`, modo 600).
// ------------------------------------------------------------------------------------------------

/// Diretório de estado do schematize no HOME (`~/.schematize`) — separado do `~/.claude`.
fn schematize_home() -> PathBuf {
    util::home_app_dir()
}

/// Caminho do arquivo de sessão.
fn auth_path() -> PathBuf {
    schematize_home().join("auth.json")
}

/// Grava a sessão com permissão 600 (diretório 700). Best-effort no chmod (não falha por isso).
pub fn save_tokens(t: &Tokens) -> Result<(), String> {
    let dir = schematize_home();
    fs::create_dir_all(&dir).map_err(|e| format!("falha ao criar {}: {e}", dir.display()))?;
    crate::util::definir_modo(&dir, 0o700);
    let body = serde_json::to_string_pretty(t).map_err(|e| e.to_string())?;
    let p = auth_path();
    fs::write(&p, body).map_err(|e| format!("falha ao gravar {}: {e}", p.display()))?;
    crate::util::definir_modo(&p, 0o600);
    Ok(())
}

/// Lê a sessão persistida (None se não existe/ inválida — nunca panica).
pub fn load_tokens() -> Option<Tokens> {
    let body = fs::read_to_string(auth_path()).ok()?;
    serde_json::from_str(&body).ok()
}

/// Apaga a sessão (logout). Idempotente — silencioso se já não existe.
pub fn logout() {
    let _ = fs::remove_file(auth_path());
}

/// Está logado? (Há uma sessão persistida, mesmo que o access precise de refresh.)
pub fn is_logged_in() -> bool {
    load_tokens().is_some()
}

/// Subject do usuário logado (identidade estável), se houver sessão.
pub fn account_sub() -> Option<String> {
    load_tokens().map(|t| t.sub)
}

// ------------------------------------------------------------------------------------------------
// Device flow — início e polling.
// ------------------------------------------------------------------------------------------------

/// Inicia o device flow: `POST {AUTH}/device_authorization`. Devolve o que a UI mostra.
pub fn device_start() -> Result<DeviceLogin, String> {
    let url = format!("{}/device_authorization", auth_base());
    let (_status, body) = post_form(&url, &[("client_id", CLIENT_ID)])?;
    parse_device_resp(&body)
}

/// Faz UMA tentativa de polling: `POST {AUTH}/token` (grant_type=device_code).
/// Erro de rede vira Err; respostas HTTP (200 ok / 400 erro) viram `PollResult`.
pub fn device_poll_once(device_code: &str) -> Result<PollResult, String> {
    let url = format!("{}/token", auth_base());
    let (_status, body) = post_form(
        &url,
        &[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
            ("client_id", CLIENT_ID),
        ],
    )?;
    Ok(parse_token_resp(&body))
}

// ------------------------------------------------------------------------------------------------
// Access token válido (com refresh transparente).
// ------------------------------------------------------------------------------------------------

/// Folga (s) antes do vencimento pra já considerar o access "vencendo" e renovar (evita corrida).
const EXPIRY_SKEW: u64 = 30;

/// Devolve um access token VÁLIDO: usa o atual se ainda vale; se venceu e há refresh, renova e
/// persiste; None se não logado ou se a renovação falhou. Nunca panica.
pub fn access_token() -> Option<String> {
    let t = load_tokens()?;
    if util::now_unix() + EXPIRY_SKEW < t.expires_at {
        return Some(t.access_token);
    }
    // Venceu (ou está vencendo): tenta refresh se houver refresh_token.
    if t.refresh_token.trim().is_empty() {
        return None;
    }
    let fresh = refresh(&t.refresh_token)?;
    let access = fresh.access_token.clone();
    let _ = save_tokens(&fresh);
    Some(access)
}

/// Renova a sessão: `POST {AUTH}/token` (grant_type=refresh_token). None em qualquer falha.
fn refresh(refresh_token: &str) -> Option<Tokens> {
    let url = format!("{}/token", auth_base());
    let (_status, body) = post_form(
        &url,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
        ],
    )
    .ok()?;
    match parse_token_resp(&body) {
        PollResult::Ok(t) => Some(t),
        _ => None,
    }
}

// ------------------------------------------------------------------------------------------------
// Parsing PURO das respostas do IdP (testável sem rede).
// ------------------------------------------------------------------------------------------------

/// Forma bruta do `/device_authorization`.
#[derive(Deserialize)]
struct DeviceResp {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default = "default_expires")]
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_expires() -> u64 {
    900
}
fn default_interval() -> u64 {
    5
}

/// Faz o parse da resposta do device flow. Erro claro se o JSON não bate (campo faltando).
pub fn parse_device_resp(json: &str) -> Result<DeviceLogin, String> {
    let r: DeviceResp = serde_json::from_str(json)
        .map_err(|e| format!("resposta inválida do device_authorization: {e}"))?;
    let complete = r
        .verification_uri_complete
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| r.verification_uri.clone());
    Ok(DeviceLogin {
        user_code: r.user_code,
        verification_uri: r.verification_uri,
        verification_uri_complete: complete,
        device_code: r.device_code,
        interval: r.interval.max(1),
        expires_in: r.expires_in.max(1),
    })
}

/// Forma bruta do `/token` — sucesso (tokens) OU erro (`error`/`error_description`) na MESMA struct.
#[derive(Deserialize)]
struct TokenResp {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Parse do `/token` usando o relógio atual pro `expires_at` (wrapper impuro do puro abaixo).
pub fn parse_token_resp(json: &str) -> PollResult {
    parse_token_at(json, util::now_unix())
}

/// Parse PURO do `/token` com `now` explícito (testável). Mapeia sucesso→Ok(Tokens) e cada
/// `error` conhecido pro `PollResult` certo. Body inesperado/ilegível → Pending (o laço tem
/// deadline próprio, então não trava eternamente).
pub fn parse_token_at(json: &str, now: u64) -> PollResult {
    let r: TokenResp = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return PollResult::Pending,
    };
    // Sucesso: tem access_token.
    if let Some(access) = r.access_token.filter(|s| !s.is_empty()) {
        let expires_in = r.expires_in.unwrap_or(3600);
        return PollResult::Ok(Tokens {
            access_token: access,
            refresh_token: r.refresh_token.unwrap_or_default(),
            expires_at: now.saturating_add(expires_in),
            sub: r.sub.unwrap_or_default(),
        });
    }
    // Erro OAuth: decide continuar/parar pelo código.
    match r.error.as_deref() {
        Some("authorization_pending") => PollResult::Pending,
        Some("slow_down") => PollResult::SlowDown,
        Some("expired_token") => PollResult::Expired,
        Some("access_denied") | Some("invalid_grant") => PollResult::Denied,
        // Erro desconhecido → para (não faz sentido insistir num erro que não sabemos tratar).
        Some(_) => PollResult::Denied,
        // Sem token e sem erro: resposta estranha — trata como pendente (deadline protege).
        None => PollResult::Pending,
    }
}

// ------------------------------------------------------------------------------------------------
// HTTP (curl) — padrão de rede da casa. Sem crate de HTTP.
// ------------------------------------------------------------------------------------------------

/// POST `application/x-www-form-urlencoded` via curl. Devolve `(status_http, corpo)`.
/// NÃO usa `-f`: precisa do corpo do erro (o IdP manda `{error,...}` com HTTP 400). Falha de
/// REDE (curl sai != 0) vira Err. `--data-urlencode` cuida do escaping de cada campo.
fn post_form(url: &str, fields: &[(&str, &str)]) -> Result<(u16, String), String> {
    let mut args: Vec<String> = vec![
        "-s".into(),
        "-m".into(),
        "20".into(),
        "-H".into(),
        "User-Agent: schematize-cli".into(),
        // Anexa "\n<http_code>" ao fim do corpo pra recuperarmos o status.
        "-w".into(),
        "\n%{http_code}".into(),
    ];
    for (k, v) in fields {
        args.push("--data-urlencode".into());
        args.push(format!("{k}={v}"));
    }
    args.push(url.into());
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let raw = util::run("curl", &arg_refs)?;
    Ok(split_status(&raw))
}

/// Separa o `(status, corpo)` da saída de `curl -w "\n%{http_code}"`: a ÚLTIMA linha é o código.
fn split_status(raw: &str) -> (u16, String) {
    let raw = raw.strip_suffix('\n').unwrap_or(raw);
    match raw.rfind('\n') {
        Some(i) => {
            let code = raw[i + 1..].trim().parse::<u16>().unwrap_or(0);
            (code, raw[..i].to_string())
        }
        None => {
            // Corpo vazio: só veio o código.
            (raw.trim().parse::<u16>().unwrap_or(0), String::new())
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Testes — parsing puro + refresh com token sintético (sem rede).
// ------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_resp_completo() {
        let j = r#"{
            "device_code":"dev-123",
            "user_code":"WDJB-MJHT",
            "verification_uri":"https://auth.schematize.org/device",
            "verification_uri_complete":"https://auth.schematize.org/device?code=WDJB-MJHT",
            "expires_in":600,
            "interval":5
        }"#;
        let d = parse_device_resp(j).expect("parse ok");
        assert_eq!(d.user_code, "WDJB-MJHT");
        assert_eq!(d.device_code, "dev-123");
        assert_eq!(d.verification_uri, "https://auth.schematize.org/device");
        assert!(d.verification_uri_complete.contains("code=WDJB-MJHT"));
        assert_eq!(d.interval, 5);
        assert_eq!(d.expires_in, 600);
    }

    #[test]
    fn device_resp_sem_complete_cai_no_uri() {
        // Sem verification_uri_complete e sem interval/expires → usa defaults e o uri simples.
        let j = r#"{
            "device_code":"d","user_code":"U","verification_uri":"https://x/device"
        }"#;
        let d = parse_device_resp(j).expect("parse ok");
        assert_eq!(d.verification_uri_complete, "https://x/device");
        assert_eq!(d.interval, 5);
        assert_eq!(d.expires_in, 900);
    }

    #[test]
    fn device_resp_invalido_e_erro() {
        assert!(parse_device_resp("{}").is_err());
        assert!(parse_device_resp("nao é json").is_err());
    }

    #[test]
    fn token_ok_calcula_expires_at() {
        let j = r#"{
            "token_type":"Bearer","access_token":"AT","refresh_token":"RT",
            "expires_in":3600,"sub":"user-42","sid":"s","aal":"aal1","amr":["pwd"]
        }"#;
        match parse_token_at(j, 1_000) {
            PollResult::Ok(t) => {
                assert_eq!(t.access_token, "AT");
                assert_eq!(t.refresh_token, "RT");
                assert_eq!(t.sub, "user-42");
                assert_eq!(t.expires_at, 1_000 + 3600);
            }
            _ => panic!("esperava Ok(Tokens)"),
        }
    }

    #[test]
    fn token_erros_mapeados() {
        let mk = |e: &str| format!(r#"{{"error":"{e}","error_description":"x"}}"#);
        assert!(matches!(parse_token_at(&mk("authorization_pending"), 0), PollResult::Pending));
        assert!(matches!(parse_token_at(&mk("slow_down"), 0), PollResult::SlowDown));
        assert!(matches!(parse_token_at(&mk("expired_token"), 0), PollResult::Expired));
        assert!(matches!(parse_token_at(&mk("access_denied"), 0), PollResult::Denied));
        assert!(matches!(parse_token_at(&mk("invalid_grant"), 0), PollResult::Denied));
        // Erro desconhecido → para (Denied).
        assert!(matches!(parse_token_at(&mk("teapot"), 0), PollResult::Denied));
    }

    #[test]
    fn token_body_estranho_vira_pending() {
        assert!(matches!(parse_token_at("nao json", 0), PollResult::Pending));
        assert!(matches!(parse_token_at("{}", 0), PollResult::Pending));
    }

    #[test]
    fn split_status_separa_codigo() {
        let (code, body) = split_status("{\"a\":1}\n400");
        assert_eq!(code, 400);
        assert_eq!(body, "{\"a\":1}");
        // Corpo com quebras internas: só a última linha é o código.
        let (code, body) = split_status("linha1\nlinha2\n200");
        assert_eq!(code, 200);
        assert_eq!(body, "linha1\nlinha2");
        // Só código, corpo vazio.
        let (code, body) = split_status("204");
        assert_eq!(code, 204);
        assert_eq!(body, "");
    }

    #[test]
    fn env_override_apara_barra() {
        std::env::set_var("SCHEMATIZE_TEST_URL_X", "https://ex.test/");
        assert_eq!(env_or("SCHEMATIZE_TEST_URL_X", "def"), "https://ex.test");
        std::env::remove_var("SCHEMATIZE_TEST_URL_X");
        assert_eq!(env_or("SCHEMATIZE_TEST_URL_X", "https://d/"), "https://d");
    }

    #[test]
    fn tokens_roundtrip_serde() {
        // Sanidade: o formato persistido lê de volta igual (o que access_token/load_tokens usam).
        let t = Tokens {
            access_token: "AT".into(),
            refresh_token: "RT".into(),
            expires_at: 123,
            sub: "s".into(),
        };
        let s = serde_json::to_string(&t).unwrap();
        let back: Tokens = serde_json::from_str(&s).unwrap();
        assert_eq!(back.access_token, "AT");
        assert_eq!(back.expires_at, 123);
        assert_eq!(back.sub, "s");
    }
}
