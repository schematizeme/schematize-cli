//! `schematize conectores`: a ponte para a lib `schematize-connectors` (ADR-0028).
//!
//! **O quê:** prova que o hub DELEGA (list/preview/enable/disable/anuncios/diag), que a prévia é
//! só leitura e offline, que anúncio nunca aparece em `--json`/sem tty/opt-out/contexto de
//! agente, que `enable` sem `--aplicar` só imprime (o `claude` fake nunca é chamado) e que os
//! exit codes da lib saem do processo sem remapear.
//!
//! **Onde:** gate local (`cargo test`). NENHUM teste usa rede: o `vivo` é um fake que falha se
//! for chamado, e o binário real só roda comandos que terminam antes de qualquer `curl`.

use connectors::anuncio::Data;
use connectors::cli::claude::{Claude, Execucao};
use connectors::conector::Conector;
use connectors::mcp::{ErroMcp, Previa, Previador};
use connectors::segredo::Segredo;
use schematize::conectores::{contexto_de_agente, executar_com, Mundo};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Prévia viva falsa: qualquer chamada é rede indevida e derruba o teste.
struct SemRede;
impl Previador for SemRede {
    /// **O quê:** panic — a prévia ao vivo não pode ser usada nestes testes.
    /// **Onde:** `Mundo.vivo` de todos os testes em processo.
    fn previa(&self, _c: &Conector, _s: Option<&Segredo>) -> Result<Previa, ErroMcp> {
        panic!("rede indevida: a prévia ao vivo foi chamada num teste");
    }
}

/// `claude` falso: conta quantas vezes foi acionado.
#[derive(Default)]
struct ContaClaude {
    chamadas: Cell<u32>,
}
impl Claude for ContaClaude {
    /// **O quê:** diz que o `claude` existe. **Onde:** checagem prévia do `--aplicar`.
    fn disponivel(&self) -> bool {
        true
    }
    /// **O quê:** registra a chamada. **Onde:** só `--aplicar` chega aqui.
    fn rodar(&self, _args: &[String]) -> Result<Execucao, String> {
        self.chamadas.set(self.chamadas.get() + 1);
        Ok(Execucao { codigo: 0, saida: String::new() })
    }
}

/// **O quê:** roda `schematize conectores <args>` em processo; devolve (exit, stdout, stderr).
/// **Onde:** testes. `cfg` vazio isola o opt-out por arquivo do HOME real de quem roda o teste.
fn rodar(
    args: &[&str],
    env: &[(&str, &str)],
    tty: bool,
    cl: &ContaClaude,
) -> (i32, String, String) {
    let cfg = std::env::temp_dir().join(format!("sch-cfg-vazio-{}", std::process::id()));
    let mut mapa: Vec<(String, String)> =
        vec![("XDG_CONFIG_HOME".into(), cfg.to_string_lossy().into_owned())];
    mapa.extend(env.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    let get = move |k: &str| mapa.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let m = Mundo {
        env: &get,
        tty,
        hoje: Data { ano: 2026, mes: 11, dia: 21 },
        vivo: &SemRede,
        claude: cl,
    };
    let resto: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = executar_com(&resto, &m, &mut o, &mut e);
    (code, String::from_utf8_lossy(&o).into_owned(), String::from_utf8_lossy(&e).into_owned())
}

fn sim(args: &[&str], env: &[(&str, &str)], tty: bool) -> (i32, String, String) {
    rodar(args, env, tty, &ContaClaude::default())
}

#[test]
fn list_delega_a_lib_e_o_principal_vem_primeiro() {
    let (c, o, _) = sim(&["list", "--lang", "pt"], &[], false);
    assert_eq!(c, 0);
    let primeira = o.lines().nth(1).expect("linha do principal");
    assert!(primeira.starts_with('★') && primeira.contains("nebularws"), "{o}");
    assert!(o.contains("asender") && o.contains("canverly"));
}

#[test]
fn list_json_e_estavel_e_nunca_traz_anuncio() {
    let (c, o, _) = sim(&["list", "--json"], &[], true);
    assert_eq!(c, 0);
    let v: serde_json::Value = serde_json::from_str(o.trim()).expect("JSON válido");
    assert_eq!(v["ok"], true);
    assert_eq!(v["conectores"][0]["id"], "nebularws");
    assert!(!o.contains("Anúncio") && !o.contains("Ad ·") && !o.contains("da casa"));
}

#[test]
fn preview_estatico_e_offline_e_somente_leitura() {
    let (c, o, _) = sim(&["preview", "asender", "--lang", "pt"], &[], false);
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("32 ferramentas") && o.contains("EMBUTIDA"), "{o}");
    let (_, j, _) = sim(&["preview", "asender", "--json"], &[], true);
    let v: serde_json::Value = serde_json::from_str(j.trim()).unwrap();
    assert_eq!(v["origem"], "estatica");
    assert_eq!(v["total"], 32);
    assert_eq!(v["escrita"], 3);
}

#[test]
fn enable_sem_aplicar_so_imprime_e_nunca_chama_o_claude() {
    let cl = ContaClaude::default();
    let (c, o, _) = rodar(&["enable", "asender", "--lang", "en"], &[], false, &cl);
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("claude mcp add") && o.contains("ASENDER_API_KEY"), "{o}");
    assert_eq!(cl.chamadas.get(), 0, "sem --aplicar nada pode ser executado");
}

#[test]
fn enable_nao_ecoa_segredo_do_ambiente() {
    let chave = "sk_live_SEGREDO_NUNCA_NA_SAIDA_123456";
    let (c, o, e) = sim(&["enable", "asender"], &[("ASENDER_API_KEY", chave)], false);
    assert_eq!(c, 0);
    assert!(!o.contains(chave) && !e.contains(chave), "segredo vazou");
}

#[test]
fn disable_sem_aplicar_so_imprime() {
    let cl = ContaClaude::default();
    let (c, o, _) = rodar(&["disable", "asender"], &[], false, &cl);
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("claude mcp remove"), "{o}");
    assert_eq!(cl.chamadas.get(), 0);
}

#[test]
fn anuncio_so_aparece_em_terminal_humano() {
    let com = sim(&["list", "--lang", "pt"], &[], true).1;
    assert_eq!(com.matches("Anúncio · da casa").count(), 1, "exatamente um: {com}");
    assert!(!sim(&["list", "--lang", "pt"], &[], false).1.contains("Anúncio"), "sem tty");
    assert!(!sim(&["list", "--sem-anuncios"], &[], true).1.contains("Ad ·"), "flag");
    let optout = sim(&["list", "--lang", "pt"], &[("SCHEMATIZE_SEM_ANUNCIOS", "1")], true).1;
    assert!(!optout.contains("Anúncio"), "opt-out por env");
}

#[test]
fn contexto_de_agente_nunca_ve_anuncio_mesmo_com_tty() {
    for var in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CURSOR_AGENT", "GEMINI_CLI"] {
        let o = sim(&["list", "--lang", "pt"], &[(var, "1")], true).1;
        assert!(!o.contains("Anúncio") && !o.contains("Ad ·"), "{var} deixou passar anúncio");
    }
    // Self-check: sem o sinal de agente o mesmo cenário MOSTRA o anúncio (o teste não é cego).
    assert!(sim(&["list", "--lang", "pt"], &[], true).1.contains("Anúncio"));
    let so = |k: &str| (k == "CLAUDECODE").then(|| "1".to_string());
    assert!(contexto_de_agente(&so));
    assert!(!contexto_de_agente(&|_: &str| None));
    assert!(!contexto_de_agente(&|_: &str| Some("  ".to_string())));
}

#[test]
fn anuncios_audita_e_json_nao_traz_o_texto() {
    let (c, o, _) = sim(&["anuncios", "--json"], &[], true);
    assert_eq!(c, 0);
    let v: serde_json::Value = serde_json::from_str(o.trim()).unwrap();
    assert!(v["slot"].is_string());
    assert!(!o.contains("Anúncio · da casa") && !o.contains("Ad · from us"));
    assert_eq!(sim(&["anuncios", "--slot", "tela-inicial"], &[], true).0, 0);
}

#[test]
fn diag_responde() {
    let (c, o, _) = sim(&["diag", "--json"], &[], false);
    assert_eq!(c, 0);
    assert!(serde_json::from_str::<serde_json::Value>(o.trim()).is_ok(), "{o}");
}

#[test]
fn exit_codes_da_lib_sao_preservados_em_processo() {
    assert_eq!(sim(&["preview", "nao-existe"], &[], false).0, 1, "id desconhecido");
    assert_eq!(sim(&["preview", "nebularws"], &[], false).0, 3, "endpoint pendente");
    assert_eq!(sim(&["preview", "canverly"], &[], false).0, 3, "sem MCP");
    assert_eq!(sim(&["enable", "asender", "--cliente", "cursor", "--aplicar"], &[], false).0, 1);
    assert_eq!(sim(&[], &[], false).0, 2, "sem subcomando");
    assert_eq!(sim(&["list", "--nao-existe"], &[], false).0, 2, "flag inválida");
}

#[test]
fn ajuda_da_lib_se_apresenta_como_schematize_conectores() {
    let (c, o, _) = sim(&["--help"], &[], false);
    assert_eq!(c, 0);
    assert!(o.contains("schematize conectores"), "usage deve citar o comando do hub: {o}");
    assert!(o.contains("preview") && o.contains("--aplicar"));
}

// ---- Binário real: o que o usuário digita (clap do hub + exit do processo). ----

/// **O quê:** HOME descartável sob o alvo do cargo. **Onde:** testes do binário.
fn home(nome: &str) -> PathBuf {
    let h = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("teste-conectores")
        .join(format!("{}-{nome}", std::process::id()));
    std::fs::create_dir_all(&h).expect("HOME de teste");
    h
}

/// **O quê:** roda o binário `schematize <args>` isolado; devolve (exit, stdout, stderr).
/// **Onde:** testes do binário. Sem sinais de agente e sem a chave do conector no env.
fn bin(nome: &str, args: &[&str]) -> (i32, String, String) {
    let h = home(nome);
    let o = Command::new(env!("CARGO_BIN_EXE_schematize"))
        .args(args)
        .env("HOME", &h)
        .env("XDG_CONFIG_HOME", h.join(".config"))
        .env_remove("ASENDER_API_KEY")
        .env_remove("SCHEMATIZE_SEM_ANUNCIOS")
        .stdin(Stdio::null())
        .output()
        .expect("sobe o schematize");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn binario_list_e_alias_connectors_dao_a_mesma_saida() {
    let (c1, a, _) = bin("alias-a", &["conectores", "list", "--json"]);
    let (c2, b, _) = bin("alias-b", &["connectors", "list", "--json"]);
    assert_eq!((c1, c2), (0, 0));
    assert_eq!(a, b);
    assert!(a.contains("\"nebularws\""));
}

#[test]
fn binario_help_do_subcomando_e_o_da_lib_e_o_do_hub_lista_o_comando() {
    let (c, o, _) = bin("help-sub", &["conectores", "--help"]);
    assert_eq!(c, 0);
    assert!(o.contains("schematize conectores") && o.contains("preview"), "{o}");
    let (_, h, _) = bin("help-hub", &["--help"]);
    assert!(h.contains("conectores"), "o --help do hub deve listar o comando: {h}");
}

#[test]
fn binario_propaga_exit_codes_sem_rede() {
    assert_eq!(bin("ec1", &["conectores", "preview", "x"]).0, 1);
    assert_eq!(bin("ec2", &["conectores"]).0, 2);
    assert_eq!(bin("ec3", &["conectores", "preview", "nebularws"]).0, 3);
    // Sem a chave no env a lib recusa ANTES de qualquer curl: exit 4, sem rede.
    let (c, o, e) = bin("ec4", &["conectores", "preview", "asender", "--ao-vivo"]);
    assert_eq!(c, 4, "{o}{e}");
    assert!(e.contains("ASENDER_API_KEY"), "cita só o NOME da variável: {e}");
}

#[test]
fn binario_sem_tty_nao_mostra_anuncio() {
    // `Command::output` captura o stdout: não é tty, então nunca há anúncio.
    let (c, o, _) = bin("sem-tty", &["conectores", "list", "--lang", "en"]);
    assert_eq!(c, 0);
    assert!(!o.contains("Ad ·") && !o.contains("Anúncio"), "{o}");
}
