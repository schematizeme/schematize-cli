//! Helpers de terminal usados por vários comandos do hub.
//!
//! **O quê:** confirmação interativa e canonicalização de caminho.
//!
//! **Onde:** `overdev`, `diversos` e o que mais precisar perguntar antes de agir.
//!
//! ## Por que estão aqui, e não onde estavam
//!
//! Moravam em `cli/ssh.rs` — por acidente de história, porque foi lá que nasceram. Quando o
//! `ssh` foi delegado ao `schematize-deployer` (ADR-0010, enfim cumprido), o arquivo inteiro
//! virou código morto, e estes dois helpers, que nunca tiveram nada de SSH, teriam ido junto.
//!
//! Um helper genérico escondido dentro de um módulo de domínio é uma armadilha para o dia em
//! que aquele domínio sair: ou ele é apagado com o resto, ou obriga a manter o módulo vivo só
//! por causa dele.

use std::io::{self, BufRead, Write};

/// **O quê:** confirmação interativa (y/N). **Onde:** antes de qualquer passo destrutivo.
///
/// **Falha fechada:** erro, EOF ou qualquer coisa diferente de "sim" devolve `false`. Num pipe
/// sem entrada, o silêncio vira NÃO — o contrário faria um comando destrutivo rodar sozinho.
pub(crate) fn confirm(prompt: &str) -> bool {
    print!("{prompt} ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes" | "s" | "sim")
}

/// **O quê:** caminho relativo vira absoluto; se não der, devolve o literal.
/// **Onde:** onde um caminho vai ser mostrado ou gravado e precisa ser inequívoco.
///
/// O fallback é o literal e não um erro: um caminho que ainda não existe é caso legítimo
/// (gravar ali é justamente o que vem depois), e falhar aqui bloquearia isso.
pub(crate) fn canon_or(path: &str) -> String {
    std::fs::canonicalize(path)
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| path.to_string())
}
