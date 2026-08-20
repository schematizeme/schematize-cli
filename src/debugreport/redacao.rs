//! REDAÇÃO de segredo — a barreira que faz o relatório ser compartilhável.
//! Puro (string -> string) e testado: token, chave privada, JWT, valor de env sensível.

/// Placeholder que substitui qualquer segredo detectado.
pub(crate) const RED: &str = "<REDIGIDO>";

/// Redige segredos de um texto qualquer vindo de env/arquivos/comandos. Cobre:
/// - tokens por prefixo: `re_…`, `sk-…`, `ghp_/gho_/ghu_/ghs_/ghr_/github_pat_…`, `xox[bap]-…`, `xapp-…`
/// - JWTs `eyJ….….…` (3 partes base64url)
/// - `Bearer <token>` (o token vira `<REDIGIDO>`)
/// - blocos `-----BEGIN … PRIVATE KEY-----` … `-----END …-----` (o bloco todo some)
/// - `NOME=valor` quando NOME contém KEY/TOKEN/SECRET/PASS/CRED, OU o valor parece um token
/// Idempotente e best-effort — na dúvida, redige (segurança-primeiro).
pub fn scrub(s: &str) -> String {
    // 1) Blocos de chave privada primeiro (são multi-linha).
    let s = redact_private_key_blocks(s);
    // 2) Linha a linha, palavra a palavra.
    let mut out: Vec<String> = Vec::new();
    for line in s.lines() {
        out.push(scrub_line(line));
    }
    out.join("\n")
}

/// Some com o miolo de qualquer bloco PEM de chave PRIVADA (defesa extra — nós nunca
/// lemos ~/.ssh, mas se algum comando cuspir um bloco, ele não passa).
pub(crate) fn redact_private_key_blocks(s: &str) -> String {
    if !s.contains("PRIVATE KEY-----") {
        return s.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    let mut in_key = false;
    for l in s.lines() {
        if !in_key && l.contains("-----BEGIN") && l.contains("PRIVATE KEY-----") {
            in_key = true;
            out.push(format!("{RED} (bloco de chave privada)"));
            continue;
        }
        if in_key {
            if l.contains("-----END") {
                in_key = false;
            }
            continue; // descarta as linhas do bloco
        }
        out.push(l.to_string());
    }
    out.join("\n")
}

/// Redige uma única linha: quebra em segmentos (espaço vs palavra), preserva o
/// espaçamento e aplica `scrub_word` em cada palavra. Trata `Bearer <token>`.
pub(crate) fn scrub_line(line: &str) -> String {
    let mut result = String::new();
    let mut prev_bearer = false;
    for (is_ws, seg) in segments(line) {
        if is_ws {
            result.push_str(&seg);
            continue;
        }
        if prev_bearer {
            result.push_str(RED);
            prev_bearer = false;
            continue;
        }
        prev_bearer = seg.eq_ignore_ascii_case("bearer");
        result.push_str(&scrub_word(&seg));
    }
    result
}

/// Quebra a linha em segmentos alternados (whitespace, não-whitespace), preservando tudo.
pub(crate) fn segments(line: &str) -> Vec<(bool, String)> {
    let mut segs: Vec<(bool, String)> = Vec::new();
    let mut cur = String::new();
    let mut cur_ws: Option<bool> = None;
    for ch in line.chars() {
        let ws = ch.is_whitespace();
        match cur_ws {
            Some(p) if p == ws => cur.push(ch),
            Some(p) => {
                segs.push((p, std::mem::take(&mut cur)));
                cur.push(ch);
                cur_ws = Some(ws);
            }
            None => {
                cur.push(ch);
                cur_ws = Some(ws);
            }
        }
    }
    if let Some(p) = cur_ws {
        segs.push((p, cur));
    }
    segs
}

/// Redige UMA palavra (sem espaços): pares `NOME=valor` sensíveis e tokens soltos.
pub(crate) fn scrub_word(word: &str) -> String {
    // Par NOME=valor: redige o valor se o NOME é sensível OU o valor parece um token.
    if let Some(eq) = word.find('=') {
        let left = &word[..eq];
        let right = &word[eq + 1..];
        if !right.is_empty() && (key_is_secret(left) || looks_like_secret_token(right)) {
            return format!("{left}={RED}");
        }
    }
    // Token solto (com ou sem pontuação ao redor).
    if looks_like_secret_token(word) {
        return RED.to_string();
    }
    word.to_string()
}

/// O NOME (lado esquerdo do `=`) indica segredo? Substring case-insensitive dos gatilhos.
pub(crate) fn key_is_secret(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASS", "CRED"].iter().any(|k| up.contains(k))
}

/// A palavra CONTÉM algo que parece um token/segredo (prefixo conhecido + charset suficiente, ou JWT)?
pub(crate) fn looks_like_secret_token(w: &str) -> bool {
    // JWT em qualquer posição.
    if let Some(idx) = w.find("eyJ") {
        if is_jwt(&w[idx..]) {
            return true;
        }
    }
    // Prefixos conhecidos, seguidos de >=8 chars do charset do token.
    const PREFIXES: &[&str] = &[
        "re_", "sk-", "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_", "xoxb-", "xoxa-",
        "xoxp-", "xapp-",
    ];
    for pfx in PREFIXES {
        if let Some(idx) = w.find(pfx) {
            let after = &w[idx + pfx.len()..];
            let n = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count();
            if n >= 8 {
                return true;
            }
        }
    }
    false
}

/// A fatia começa com um JWT `eyJ…`.`…`.`…` (3 partes base64url não-vazias)?
pub(crate) fn is_jwt(s: &str) -> bool {
    let run: String = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .collect();
    let parts: Vec<&str> = run.split('.').collect();
    if parts.len() < 3 || !parts[0].starts_with("eyJ") {
        return false;
    }
    parts
        .iter()
        .take(3)
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
}
