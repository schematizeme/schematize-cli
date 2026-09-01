//! `schematize debug --collect` — COLETOR DE DEBUG.
//! O quê: junta num único relatório de texto TUDO que ajuda a diagnosticar a
//! ferramenta na máquina de outro usuário (sistema, instalação, PATH, dependências,
//! config, skills, overdev, updater, doctor, logs) pra ele compartilhar.
//! Onde: `schematize debug --collect [--out <path>] [--stdout]`.
//!
//! PRIORIDADE Nº1 — NUNCA VAZAR SEGREDO. Duas camadas de defesa:
//!  1) EVITAÇÃO: NUNCA lê o conteúdo de `~/.schematize/auth.json` (token de sessão),
//!     de `~/.ssh/*` (chaves privadas), nem de qualquer arquivo de chave de API. O
//!     `~/.schematize/` é listado só por NOME+TAMANHO; a sessão é reportada como
//!     "logado sim/não" + o `sub` (id interno, não é segredo).
//!  2) REDAÇÃO: `scrub()` é aplicada ao relatório INTEIRO no fim — qualquer token
//!     (re_/sk-/ghp_/xox…/JWT/Bearer), bloco de chave privada, ou par
//!     `KEY=/TOKEN=/SECRET=/PASS…=` (e toda var de ambiente cujo NOME contenha
//!     KEY/TOKEN/SECRET/PASS/CRED) vira `<REDIGIDO>` antes de sair.
//!
//! Postura: TUDO best-effort — nunca panica; a falha de uma seção vira
//! "(indisponível: <motivo>)" e o resto do relatório segue.

use crate::{account, config, debug, doctor, overdev, registry, skills, util};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

// Submódulos (piso da casa: <=750 linhas, uma unidade lógica por arquivo).
mod fmt;
mod redacao;
mod secoes;
mod sonda;
use fmt::*;
pub use redacao::*;
use secoes::*;
use sonda::*;

// ================================================================================================
// REDAÇÃO (scrub) — a rede de segurança. Sem crate de regex: varredura à mão (estilo da casa).
// ================================================================================================

// ================================================================================================
// COLETA — monta o relatório seção a seção. Tudo best-effort.
// ================================================================================================

/// Monta o relatório COMPLETO (texto). Aplica `scrub` no fim, como rede de segurança
/// sobre tudo que entrou de env/arquivo/comando.
/// Monta o relatório. `online=false` (default) é OFFLINE-first e RÁPIDO — pula as seções que
/// batem na rede (updater/rate-limit do GitHub, alcance do catálogo, doctor/github_reachable),
/// que podem TRAVAR numa máquina com rede bloqueada/lenta (curl sem timeout curto). Com
/// `online=true` inclui esses diagnósticos de rede (úteis pra bug de versionamento).
pub fn collect(online: bool) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "===== SCHEMATIZE DEBUG REPORT =====");
    let _ = writeln!(o, "gerado em: {} (epoch {})", fmt_epoch(util::now_unix()), util::now_unix());
    let _ = writeln!(
        o,
        "modo: {}",
        if online { "online (inclui rede)" } else { "offline (rápido; use --online p/ rede)" }
    );
    let _ =
        writeln!(o, "AVISO: segredos são redigidos automaticamente; revise antes de compartilhar.");

    sec_sistema(&mut o);
    sec_hardware(&mut o);
    sec_instalacao(&mut o);
    sec_path_env(&mut o);
    sec_dependencias(&mut o);
    sec_config(&mut o);
    sec_skills(&mut o, online);
    let overdev_roots = sec_overdev(&mut o);
    if online {
        sec_updater(&mut o);
        sec_doctor(&mut o);
    } else {
        hdr(&mut o, "8-9. UPDATER + DOCTOR (rede)");
        let _ = writeln!(
            &mut o,
            "  (pulados no modo offline — rode `schematize debug --collect --online` p/ incluir)"
        );
    }
    sec_logs(&mut o, &overdev_roots);

    let _ = writeln!(o, "\n===== FIM =====");

    // Rede de segurança final: redige o relatório inteiro.
    scrub(&o)
}

/// Grava o relatório em `out` (ou `~/.schematize/debug-report-<epoch>.txt`), modo 600.
/// Retorna o caminho gravado. Cria `~/.schematize` (modo 700) se preciso.
pub fn write_report(out: Option<&Path>, online: bool) -> Result<PathBuf, String> {
    let report = collect(online);
    let path = match out {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = util::home_app_dir();
            fs::create_dir_all(&dir)
                .map_err(|e| format!("falha ao criar {}: {e}", dir.display()))?;
            crate::util::definir_modo(&dir, 0o700);
            dir.join(format!("debug-report-{}.txt", util::now_unix()))
        }
    };
    fs::write(&path, report.as_bytes())
        .map_err(|e| format!("falha ao gravar {}: {e}", path.display()))?;
    crate::util::definir_modo(&path, 0o600);
    Ok(path)
}

/// Resumo curto (pro CLI imprimir depois de gravar / pra GUI). Sem segredo.
pub fn short_summary() -> String {
    let logged = if account::is_logged_in() { "sim" } else { "não" };
    let n_skills = skills::load_state().skills.len();
    format!(
        "schematize v{} · logado: {logged} · skills instaladas: {n_skills}",
        env!("CARGO_PKG_VERSION")
    )
}

// ------------------------------------------------------------------------------------------------
// Seções.
// ------------------------------------------------------------------------------------------------

// ------------------------------------------------------------------------------------------------
// Helpers de coleta/formatação.
// ------------------------------------------------------------------------------------------------

// ================================================================================================
// TESTES — foco na scrub (o piso de segurança). Não tocam em rede/HOME.
// ================================================================================================

/// Resumo ESTRUTURADO do ambiente, pro corpo do `POST /diagnostics`.
///
/// O quê: os mesmos dados de SO/hardware da seção 1/1b, na forma ANINHADA do contrato do
/// servidor (`DiagnosticInput.env` do OpenAPI): `os{}`, `hardware{}`, `display{}`. O schema
/// aceita campos livres, mas quem CONSULTA usa os caminhos do exemplo publicado — mandar
/// plano passaria na validação e sumiria de toda query de triagem.
/// Onde: `diagnostics::send`, ao montar o corpo.
///
/// Por que existe: o `report` é um blob de texto de centenas de KB. Dá pra LER um relatório
/// nele, mas não dá pra PERGUNTAR "quantos relatos vieram de Wayland com GPU AMD na 0.50?"
/// sem reparsear tudo. Triagem sem campo filtrável vira retrabalho — que é justamente o que
/// coletar hardware deveria evitar. O blob continua indo junto, como fonte da verdade.
///
/// **Entrada:** nenhuma. **Saída:** objeto JSON com os campos acima (todos string, exceto
/// `cores`), sempre presentes — valor `"(indisponível)"` quando a sonda não conseguiu.
/// **Efeitos:** lê /proc e executa `uname`/`nproc`/`lspci` (best-effort, com timeout).
pub fn ambiente() -> serde_json::Value {
    let (os_nome, os_ver) = os_release();
    serde_json::json!({
        "os": {
            "name": os_nome,
            "version": os_ver,
            "kernel": cmd_out("uname", &["-r"]),
            "arch": std::env::consts::ARCH,
        },
        "hardware": {
            "cpu": cpu_modelo(),
            "cores": cmd_out("nproc", &[]).parse::<u32>().unwrap_or(0),
            "ram_mb": ram_total_mb_num(),
            "gpu": gpu_modelo(),
        },
        // Versões dos OUTROS binários da mesma máquina. Quase todo bug nosso é de
        // DESCASAMENTO — CLI novo com GUI velha (o `Cargo.lock` da GUI pina o crate por
        // commit), updater defasado — e sem isto a triagem gasta uma ida-e-volta só pra
        // descobrir isso. O `version` do topo do corpo é só de quem enviou.
        "app": {
            "cli": env!("CARGO_PKG_VERSION"),
            "gui": versao_de("schematize-gui"),
            "updater": versao_de("schematize-updater"),
        },
        "display": {
            "SLINT_BACKEND": getenv("SLINT_BACKEND"),
            "WAYLAND_DISPLAY": getenv("WAYLAND_DISPLAY"),
            "DISPLAY": getenv("DISPLAY"),
            "XDG_CURRENT_DESKTOP": getenv("XDG_CURRENT_DESKTOP"),
            "XDG_SESSION_TYPE": getenv("XDG_SESSION_TYPE"),
        },
    })
}

/// Versão de um binário irmão (`schematize-gui`, `schematize-updater`), ou por que não deu.
///
/// O quê: roda `<bin> --version` e devolve só o número. Onde: o bloco `app` do [`ambiente`].
/// Por que: descasamento entre os binários é a causa nº 1 de "atualizei mas abre a versão
/// velha"; saber as três versões de uma vez mata a pergunta antes dela ser feita.
/// **Saída:** o número, `"(não instalado)"` ou `"(indisponível: …)"`. **Efeitos:** executa
/// processo externo com timeout; nunca panica.
fn versao_de(bin: &str) -> String {
    if which_all(bin).is_empty() {
        return "(não instalado)".into();
    }
    let bruto = cmd_out(bin, &["--version"]);
    // Só aceita o que PARECE versão. `schematize-gui` de versões antigas ignora
    // `--version` e ABRE A JANELA; o `cmd_out` corta no timeout e devolve a 1ª linha do log
    // de ambiente, que virava "versão" no relatório. Confiar na boa vontade do outro
    // binário é o mesmo bug de sempre: aceitar saída sem conferir o formato.
    let candidato = bruto.split_whitespace().last().unwrap_or("").trim();
    if parece_versao(candidato) {
        candidato.to_string()
    } else {
        format!("(não reportou versão: {})", bruto.chars().take(40).collect::<String>())
    }
}

/// A string parece um número de versão (`0.7.5`, `1.2.3-rc1`)?
///
/// O quê: exige começar com dígito e conter só dígito/ponto/hífen/alfanumérico. Onde:
/// [`versao_de`]. **Efeitos:** nenhum.
fn parece_versao(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
}

// Os módulos de teste ficam NO FIM do arquivo, sempre.
//
// Nove testes de conformidade/pentest definem "código de produção" como
// `fonte.split("#[cfg(test)]").next()`. Código abaixo do primeiro `#[cfg(test)]` some
// dessas varreduras — e aqui embaixo moravam `ambiente`, `versao_de` e `parece_versao`,
// que montam o relatório enviado ao suporte. Invisível pro scanner é onde um vazamento
// passaria despercebido. `teste_no_fim_do_arquivo` (tests/vps_conformidade.rs) trava isso.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(crate) fn scrub_redige_token_por_prefixo() {
        assert_eq!(scrub("re_abc12345XYZ"), RED);
        assert_eq!(scrub("sk-abcdefgh12345"), RED);
        assert_eq!(scrub("ghp_0123456789abcdef"), RED);
        assert_eq!(scrub("xoxb-1234567890-abcdef"), RED);
        // Curto demais depois do prefixo NÃO é tratado como token.
        assert_eq!(scrub("re_abc"), "re_abc");
    }

    #[test]
    pub(crate) fn scrub_redige_jwt() {
        let jwt =
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert_eq!(scrub(jwt), RED);
        // JWT embutido numa frase: some, o resto fica.
        let out = scrub(&format!("token: {jwt} fim"));
        assert!(out.contains("token:"));
        assert!(out.contains("fim"));
        assert!(!out.contains("eyJ"));
        assert!(out.contains(RED));
    }

    #[test]
    pub(crate) fn scrub_redige_bearer() {
        let out = scrub("Authorization: Bearer abc123def456ghi");
        assert!(out.contains("Bearer <REDIGIDO>"), "got: {out}");
        assert!(!out.contains("abc123def456ghi"));
    }

    #[test]
    pub(crate) fn scrub_redige_key_value() {
        assert_eq!(scrub("API_KEY=supersecretvalue"), "API_KEY=<REDIGIDO>");
        assert_eq!(scrub("MY_TOKEN=abc"), "MY_TOKEN=<REDIGIDO>");
        assert_eq!(scrub("DB_PASSWORD=hunter2"), "DB_PASSWORD=<REDIGIDO>");
        assert_eq!(scrub("AWS_SECRET_ACCESS_KEY=zzz"), "AWS_SECRET_ACCESS_KEY=<REDIGIDO>");
        assert_eq!(scrub("MY_CRED=x"), "MY_CRED=<REDIGIDO>");
        // Nome com sensível preserva o NOME, redige só o valor.
        let out = scrub("export GITHUB_TOKEN=ghp_realtokenvalue1234");
        assert!(out.contains("GITHUB_TOKEN=<REDIGIDO>"), "got: {out}");
        assert!(!out.contains("ghp_realtokenvalue1234"));
    }

    #[test]
    pub(crate) fn scrub_preserva_texto_normal() {
        assert_eq!(scrub("hello world"), "hello world");
        assert_eq!(scrub("TERM=xterm-256color"), "TERM=xterm-256color");
        assert_eq!(scrub("LANG=en_US.UTF-8"), "LANG=en_US.UTF-8");
        assert_eq!(scrub("versao 0.30.0 instalada ok"), "versao 0.30.0 instalada ok");
        assert_eq!(scrub("/home/user/.cargo/bin/schematize"), "/home/user/.cargo/bin/schematize");
        // Preserva espaçamento e múltiplas linhas.
        assert_eq!(scrub("a  b\nc"), "a  b\nc");
    }

    #[test]
    pub(crate) fn scrub_redige_bloco_de_chave_privada() {
        let pem = "antes\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAAB3Nz...\nsecretline\n-----END OPENSSH PRIVATE KEY-----\ndepois";
        let out = scrub(pem);
        assert!(out.contains("antes"));
        assert!(out.contains("depois"));
        assert!(!out.contains("secretline"));
        assert!(!out.contains("AAAAB3Nz"));
        assert!(out.contains(RED));
    }

    #[test]
    pub(crate) fn scrub_valor_com_token_mesmo_sem_nome_sensivel() {
        // Nome NÃO sensível, mas o valor parece token → redige o valor.
        let out = scrub("foo=ghp_abcdefgh12345678");
        assert_eq!(out, "foo=<REDIGIDO>");
    }

    #[test]
    pub(crate) fn short_summary_nao_panica() {
        // Só sanidade: monta a string sem tocar em segredo.
        let s = short_summary();
        assert!(s.contains("schematize v"));
    }

    #[test]
    pub(crate) fn fmt_epoch_conhecido() {
        // 2021-01-01 00:00:00 UTC = 1609459200.
        assert_eq!(fmt_epoch(1_609_459_200), "2021-01-01 00:00:00 UTC");
    }
}

#[cfg(test)]
mod tests_ambiente {
    /// O QUE: o `env` sai na forma ANINHADA que o servidor documenta, com `ram_mb` NUMÉRICO.
    ///
    /// POR QUE trava a forma: o `DiagnosticInput.env` é `additionalProperties: true` — campos
    /// livres. Ou seja, o formato PLANO que este cliente mandava passava na validação e era
    /// aceito com 202, mas não casava com nenhuma query de triagem, porque quem consulta usa
    /// os caminhos do exemplo publicado (`env->'hardware'->>'cpu'`). Aceito ≠ útil: sem esta
    /// guarda, a regressão é silenciosa dos dois lados.
    ///
    /// `ram_mb` numérico tem motivo próprio: como string (`"31478 MB"`) o campo não responde
    /// "quais máquinas têm menos de X" — a única pergunta que se faz a ele.
    /// O QUE: só string com CARA de versão é aceita como versão.
    ///
    /// POR QUE: `schematize-gui` antigo ignora `--version` e abre a janela; o coletor corta
    /// no timeout e recebe a 1ª linha do log de ambiente. Sem este filtro, o relatório
    /// dizia que a versão da GUI era `(WAYLAND_DISPLAY=wayland-0))`.
    #[test]
    fn so_aceita_o_que_parece_versao() {
        assert!(super::parece_versao("0.7.5"));
        assert!(super::parece_versao("1.2.3-rc1"));
        assert!(!super::parece_versao("(WAYLAND_DISPLAY=wayland-0))"));
        assert!(!super::parece_versao("Terminado"));
        assert!(!super::parece_versao(""));
    }

    #[test]
    fn env_segue_a_forma_do_contrato() {
        let e = super::ambiente();

        for bloco in ["os", "hardware", "display"] {
            assert!(e.get(bloco).is_some_and(|b| b.is_object()), "falta o bloco `{bloco}`");
        }
        for (bloco, campo) in [
            ("os", "name"),
            ("os", "version"),
            ("os", "kernel"),
            ("os", "arch"),
            ("hardware", "cpu"),
            ("hardware", "cores"),
            ("hardware", "ram_mb"),
            ("hardware", "gpu"),
            ("display", "SLINT_BACKEND"),
            ("display", "WAYLAND_DISPLAY"),
            ("display", "DISPLAY"),
        ] {
            assert!(e[bloco].get(campo).is_some(), "falta `{bloco}.{campo}`");
        }
        assert!(e["hardware"]["cores"].is_number(), "cores tem que ser número");
        let ram = &e["hardware"]["ram_mb"];
        assert!(
            ram.is_number() || ram.is_null(),
            "ram_mb tem que ser NÚMERO (ou null), veio {ram}"
        );

        // Nada de campo plano legado — se voltar, as duas formas convivem e a query erra.
        for antigo in ["os_name", "os_version", "cpu", "cores", "ram_mb", "gpu"] {
            assert!(e.get(antigo).is_none(), "campo plano legado `{antigo}` de volta na raiz");
        }

        // Teto do contrato: 16 KB serializados.
        let n = serde_json::to_vec(&e).unwrap().len();
        assert!(n <= 16 * 1024, "env com {n} bytes passa do teto de 16 KB do servidor");
    }
}
