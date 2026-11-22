//! `schematize conectores` (alias `connectors`): ponte fina para a lib `schematize-connectors`.
//!
//! **O quê:** o catálogo de conectores MCP (list, preview, enable/disable, anuncios, diag) é da
//! lib (ADR-0028). Este módulo só (1) repassa os argumentos crus a `connectors::cli::executar`,
//! (2) injeta o ambiente do hub (idioma escolhido em `schematize lang`) e (3) endurece a regra
//! "anúncio nunca vai para agente": em contexto de agente o `tty` é forçado a `false`, e a lib
//! recusa anúncio sem tty (e em `--json`, e com opt-out). Nada da lógica é reimplementado.
//!
//! **Onde:** o braço `Cmd::Conectores` do `main`. Exit codes (0/1/2/3/4/5/6) são os da lib e
//! saem do processo sem remapear. A prévia é só leitura (a lib nunca chama `tools/call`); o
//! segredo nunca passa por argv (a lib lê a env var por NOME).

use connectors::anuncio::Data;
use connectors::cli::amb::{env_real, hoje_utc, stdout_e_tty, Amb, LeEnv, Reais};
use connectors::cli::claude::{Claude, ClaudeReal};
use connectors::mcp::Previador;
use std::io::Write;

/// Nome com que a lib se apresenta no `usage`/erros (argv[0]).
pub const ARGV0: &str = "schematize conectores";

/// Variáveis que o próprio agente de IA exporta no processo que ele lança. Presença (qualquer
/// valor não vazio) = a saída pode ser lida por um agente, onde anúncio é vetor de injeção.
const SINAIS_DE_AGENTE: &[&str] =
    &["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CURSOR_AGENT", "GEMINI_CLI", "CODEX_SANDBOX"];

/// O mundo externo de uma execução (tudo injetável; os testes usam fakes).
pub struct Mundo<'a> {
    /// Leitor de variável de ambiente.
    pub env: LeEnv<'a>,
    /// stdout é terminal interativo (antes do filtro de agente).
    pub tty: bool,
    /// "Hoje" (UTC), para a expiração dos anúncios.
    pub hoje: Data,
    /// Prévia ao vivo (a única que toca a rede).
    pub vivo: &'a dyn Previador,
    /// Executor do `claude` (só usado por `--aplicar`).
    pub claude: &'a dyn Claude,
}

/// **O quê:** o processo roda sob um agente de IA? **Onde:** `executar_com`, para zerar o tty.
pub fn contexto_de_agente(get: LeEnv<'_>) -> bool {
    SINAIS_DE_AGENTE.iter().any(|k| get(k).is_some_and(|v| !v.trim().is_empty()))
}

/// **O quê:** monta o argv da lib: `schematize conectores` + o que o usuário digitou.
/// **Onde:** `executar_com`.
pub fn argv_da_lib(resto: &[String]) -> Vec<String> {
    std::iter::once(ARGV0.to_string()).chain(resto.iter().cloned()).collect()
}

/// **O quê:** executa a lib com o mundo dado e devolve o exit code dela.
/// **Onde:** `executar` (real) e os testes. Idioma: sem `SCHEMATIZE_LANG` no env, vale o idioma
/// do hub (`schematize lang`) — `--lang` explícito ainda vence, a lib o trata.
pub fn executar_com(
    resto: &[String],
    m: &Mundo<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let real = m.env;
    let lang = crate::i18n::current_code();
    let env = move |k: &str| -> Option<String> {
        if k == "SCHEMATIZE_LANG" {
            return real(k).filter(|v| !v.is_empty()).or_else(|| Some(lang.clone()));
        }
        real(k)
    };
    let tty = m.tty && !contexto_de_agente(&env);
    let reais = Reais::novo();
    let amb = Amb {
        env: &env,
        tty,
        hoje: m.hoje,
        estatico: &reais.estatico,
        vivo: m.vivo,
        claude: m.claude,
    };
    connectors::cli::executar(&argv_da_lib(resto), &amb, out, err)
}

/// **O quê:** executa com o ambiente real (env, tty, curl, `claude`) e devolve o exit code.
/// **Onde:** `main`, braço `Cmd::Conectores`.
pub fn executar(resto: &[String]) -> i32 {
    let reais = Reais::novo();
    let m = Mundo {
        env: &env_real,
        tty: stdout_e_tty(),
        hoje: hoje_utc(),
        vivo: &reais.vivo,
        claude: &ClaudeReal,
    };
    executar_com(resto, &m, &mut std::io::stdout(), &mut std::io::stderr())
}
