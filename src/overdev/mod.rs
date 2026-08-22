//! Motor do modo OVERDEV: dev contínuo até o checklist fechar, à prova de parada
//! prematura e SEM travar pra perguntar (parkeia a pergunta e segue).
//! O quê: start/check/guard/status/hold/ask/stop. check = Stop hook; guard = PreToolUse
//! hook que VETA AskUserQuestion enquanto em overdev. Onde: chamado por main.

use crate::settings;
use crate::util::{self, self_exe};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

// Submódulos (piso da casa: <=750 linhas, uma unidade lógica por arquivo).
// Caixa de entrada (injetar demandas com outro agente rodando) e as primitivas de
// concorrência que ela exige. `pub` porque a GUI e o CLI as usam direto.
pub mod caixa;
pub mod resposta;
pub mod trava;

mod progresso;
mod conclusoes;
mod divisao;
mod gate;
pub mod supervisor;
mod notas;
mod arquivo;
pub use progresso::*;
pub use conclusoes::*;
pub use divisao::*;
pub use gate::*;
pub use notas::*;
pub use arquivo::*;


const DEFAULT_MAX_ITERS: u64 = 200;
const QUESTIONS_FILE: &str = "PERGUNTAS-OVERDEV.txt";

#[derive(Serialize, Deserialize)]
struct OverState {
    mode: String, // active | done | blocked | stopped
    max_iters: u64,
    objetivo: String,
    started: u64,
}

/// Dir de overdev relativo ao cwd — agora `.schematize/overdev` (com fallback ao `.overdev`
/// legado), resolvido pelo módulo central `paths`. Contrato dos hooks Stop/PreToolUse.
fn dir() -> PathBuf {
    crate::paths::overdev_dir_cwd()
}
fn state_file() -> PathBuf {
    dir().join("state.json")
}
fn checklist() -> PathBuf {
    dir().join("CHECKLIST.md")
}
fn iters_file() -> PathBuf {
    dir().join("iterations")
}

fn load() -> Option<OverState> {
    fs::read_to_string(state_file()).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Resumo pro status/GUI: (run ativo?, objetivo se ativo).
pub fn status_brief() -> (bool, Option<String>) {
    match load() {
        Some(st) if st.mode == "active" => (true, Some(st.objetivo)),
        _ => (false, None),
    }
}
// ---------------------------------------------------------------------------
// Acessores PATH-AWARE (consumidos por agentrun::run_attached e pela GUI, que
// monitoram um projeto que NÃO é o cwd do processo). As funções acima operam no
// `.overdev` relativo ao cwd (contrato dos hooks Stop/PreToolUse); estas leem o
// `.overdev` de um `root` explícito, sem efeito colateral.
// ---------------------------------------------------------------------------

/// Dir de overdev de um `root` explícito — `.schematize/overdev` com fallback ao `.overdev`
/// legado (regra "ler ambos" centralizada em `paths`).
fn dir_at(root: &Path) -> PathBuf {
    crate::paths::overdev_dir_at(root)
}

/// Carrega o state.json de um projeto arbitrário (None se ausente/ilegível).
fn load_at(root: &Path) -> Option<OverState> {
    fs::read_to_string(dir_at(root).join("state.json")).ok().and_then(|s| serde_json::from_str(&s).ok())
}






// ---------------------------------------------------------------------------
// LOG DE CONCLUSÕES: cada `- [x]` (máquina) que aparece no CHECKLIST ganha a HORA
// em que foi detectado. Como o Claude edita o CHECKLIST direto (às vezes POR FORA
// do app), a detecção é por DIFF e é acionada no check() (Stop hook, a cada turno).
// Registro em `.overdev/completions.json` (mapa text->ts, epoch secs).
// ---------------------------------------------------------------------------










fn save(st: &OverState) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    fs::write(state_file(), serde_json::to_string_pretty(st).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}












/// Inicia um run de overdev no diretório atual.
pub fn start(objetivo: &str, max_iters: Option<u64>) -> Result<(), String> {
    // Auto-migração do layout legado: se existir `.overdev/` e ainda não `.schematize/overdev/`,
    // move o control-plane pro layout novo (o run passa a operar em `.schematize/overdev/`).
    match crate::paths::migrate_legacy_overdev(Path::new(".")) {
        Ok(true) => println!("migrado: .overdev/ → .schematize/overdev/"),
        Ok(false) => {}
        Err(e) => eprintln!("aviso: falha ao migrar .overdev/ → .schematize/overdev/: {e}"),
    }
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let st = OverState {
        mode: "active".into(),
        max_iters: max_iters.unwrap_or(DEFAULT_MAX_ITERS),
        objetivo: objetivo.to_string(),
        started: util::now_unix(),
    };
    save(&st)?;
    fs::write(iters_file(), "0").map_err(|e| e.to_string())?;
    if !checklist().exists() {
        let tpl = format!(
            "# OVERDEV — checklist\n\nObjetivo: {objetivo}\n\n> Um item por linha, verificável. Máquina: `- [ ]` aberto · `- [x]` feito. Humano (só a pessoa fecha, o agente NÃO tenta): `- [H ]` aberto · `- [H x]` feito. `- [~]` on-hold (pergunta parkeada).\n\n- [ ] (gere o checklist exaustivo de MÁQUINA aqui)\n- [H ] (opcional: itens que só o humano fecha pela GUI/CLI)\n"
        );
        fs::write(checklist(), tpl).map_err(|e| e.to_string())?;
    }
    // Gitignore SÓ do operacional (`.schematize/`) — o archive (`_archive/`) NÃO é ignorado: ele é
    // um REPOSITÓRIO git próprio (privado, obrigatório), irmão dos microserviços, que documenta a
    // evolução do projeto (o `git init` do archive é feito por `ensure_archive_mirror`).
    let gi = Path::new(".gitignore");
    let cur = fs::read_to_string(gi).unwrap_or_default();
    if !cur.contains(".schematize") {
        let _ = fs::write(gi, format!("{cur}\n.schematize/\n"));
    }
    // Archive OBRIGATÓRIO (criticidade 0 — observabilidade da evolução do sistema): materializa
    // `<projeto>_archive/overdev/` e espelha os artefatos. NUNCA é opcional.
    ensure_archive_mirror(Path::new("."));
    println!("overdev ATIVO. Objetivo: {objetivo}");
    println!("Preencha .schematize/overdev/CHECKLIST.md (exaustivo). O agente não pode parar até fechar.");
    // Snapshot inicial no DB local (best-effort: nunca quebra o start se o DB falhar).
    let _ = crate::overdevdb::snapshot(Path::new("."));
    Ok(())
}














// ---------------------------------------------------------------------------
// Notas do humano: prompt de correção do overdev + pontos por task.
// Gravadas em .overdev/NOTAS.md pra a GUI ler depois. `add_note`/`read_notes`.
// ---------------------------------------------------------------------------






/// Mostra o estado atual: checklist em duas colunas (MÁQUINA vs HUMANO).
pub fn status() {
    match load() {
        None => println!("sem overdev ativo neste diretório."),
        Some(st) => {
            let c = counts();
            let it = fs::read_to_string(iters_file()).unwrap_or_else(|_| "0".into());
            println!("modo={} objetivo={}", st.mode, st.objetivo);
            println!("checklist          MÁQUINA   HUMANO");
            println!("  abertos          {:>7}  {:>7}", c.open, c.human);
            println!("  feitos           {:>7}  {:>7}", c.done_m, c.done_h);
            println!("  on-hold          {:>7}  {:>7}", c.hold, "-");
            println!("  (feitos totais: {} · faltam p/ concluir: {})", c.done(), c.open + c.human);
            println!("ciclos={} / max={}", it.trim(), st.max_iters);
            if let Ok(q) = fs::read_to_string(QUESTIONS_FILE) {
                let n = q.lines().filter(|l| l.starts_with('[')).count();
                if n > 0 {
                    println!("perguntas parkeadas: {n} (ver ./{QUESTIONS_FILE})");
                }
            }
            let notas = read_notes(Path::new("."));
            let nn = notas.lines().filter(|l| l.starts_with("## [")).count();
            if nn > 0 {
                println!("notas do humano: {nn} (ver .schematize/overdev/NOTAS.md)");
            }
            // Tokens + modelo do run (parse do transcript do Claude), best-effort.
            let proj = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let u = crate::usage::agent_usage(&proj);
            if u.messages > 0 {
                println!(
                    "tokens: {} (in {} / out {} / cache-read {}) · modelo: {}",
                    u.total,
                    u.input,
                    u.output,
                    u.cache_read,
                    u.main_model().unwrap_or("-")
                );
            }
        }
    }
}

/// Encerra o run (mode=stopped) — o hook volta a ser inerte.
pub fn stop() -> Result<(), String> {
    if let Some(mut st) = load() {
        st.mode = "stopped".into();
        save(&st)?;
    }
    println!("overdev encerrado.");
    Ok(())
}

/// Liga os hooks no settings.json (Stop + PreToolUse), apontando pro binário atual.
pub fn enable() -> Result<(), String> {
    settings::enable(&self_exe())?;
    println!("overdev habilitado (hooks Stop + PreToolUse registrados). Inerte até `schematize overdev start`.");
    Ok(())
}

/// Desliga os hooks do overdev do settings.json.
pub fn disable() -> Result<(), String> {
    settings::disable()?;
    println!("overdev desabilitado (hooks removidos do settings.json).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIX: &str = "\
# OVERDEV
- [ ] item máquina aberto A
- [x] item máquina feito B
- [~] item on-hold C
- [H ] item humano aberto D
- [H x] item humano feito E
- [H ] item humano aberto F
não é item
  - [ ] item indentado aberto G
";

    #[test]
    fn conta_as_quatro_categorias() {
        let c = count_str(FIX);
        assert_eq!(c.open, 2, "máquina-abertos: '- [ ]' (inclui indentado)");
        assert_eq!(c.human, 2, "humano-abertos: '- [H ]'");
        assert_eq!(c.hold, 1, "on-hold: '- [~]'");
        assert_eq!(c.done_m, 1, "máquina-feito: '- [x]'");
        assert_eq!(c.done_h, 1, "humano-feito: '- [H x]'");
        assert_eq!(c.done(), 2, "feitos totais = máquina + humano");
    }

    #[test]
    fn compat_checklist_antigo_sem_humano() {
        // Checklist legado: só marcadores de máquina — comportamento idêntico ao antigo.
        let s = "- [ ] a\n- [x] b\n- [~] c\n";
        let c = count_str(s);
        assert_eq!((c.open, c.done(), c.hold, c.human), (1, 1, 1, 0));
    }

    #[test]
    fn humano_aberto_nao_cai_no_ramo_de_maquina() {
        // '- [H ]' NÃO pode ser contado como '- [ ]' e '- [H x]' NÃO como '- [x]'.
        let c = count_str("- [H ] x\n- [H x] y\n");
        assert_eq!(c.open, 0);
        assert_eq!(c.done_m, 0);
        assert_eq!(c.human, 1);
        assert_eq!(c.done_h, 1);
    }

    fn dec(s: &str) -> StopDecision {
        let c = count_str(s);
        let open = items_with_marker(s, "- [ ]", 8);
        let human = items_with_marker(s, "- [H ]", 12);
        decide_stop(&c, true, &open, &human, 1, 200)
    }

    #[test]
    fn maquina_aberto_rejeita_a_parada() {
        // Há '- [ ]' → BLOCK, mesmo com humanos pendentes.
        match dec("- [ ] a\n- [H ] b\n") {
            StopDecision::Block(m) => assert!(m.contains("NÃO PARE")),
            other => panic!("esperava Block, veio {other:?}"),
        }
    }

    #[test]
    fn so_humano_aberto_permite_parar_e_informa() {
        // Zero '- [ ]', mas há '- [H ]' → permite parar E lista os humanos.
        match dec("- [x] a\n- [H ] fecha isso na GUI\n") {
            StopDecision::PermitWithHuman(m) => {
                assert!(m.contains("HUMANOS"));
                assert!(m.contains("fecha isso na GUI"));
            }
            other => panic!("esperava PermitWithHuman, veio {other:?}"),
        }
    }

    #[test]
    fn concluido_de_verdade_permite_silencioso() {
        // Zero '- [ ]' e zero '- [H ]' (on-hold não conta) → Permit.
        assert_eq!(dec("- [x] a\n- [~] b\n"), StopDecision::Permit);
    }

    #[test]
    fn gate_falhando_bloqueia_mesmo_sem_maquina_aberta() {
        let c = count_str("- [x] a\n- [H ] b\n");
        let d = decide_stop(&c, false, &[], &[], 1, 200);
        match d {
            StopDecision::Block(m) => assert!(m.contains("gate")),
            other => panic!("esperava Block por gate, veio {other:?}"),
        }
    }

    #[test]
    fn fecha_humano_por_texto() {
        let s = "- [ ] m\n- [H ] revisar copy\n- [H ] aprovar deploy\n";
        let (out, txt) = mark_human_str(s, Some("aprovar"), None).unwrap();
        assert!(txt.contains("aprovar deploy"));
        assert!(out.contains("- [H x] aprovar deploy"));
        assert!(out.contains("- [H ] revisar copy"), "só fecha o que casa");
        assert!(out.contains("- [ ] m"), "não toca item de máquina");
    }

    #[test]
    fn fecha_humano_por_indice() {
        let s = "- [H ] um\n- [H ] dois\n";
        let (out, txt) = mark_human_str(s, None, Some(2)).unwrap();
        assert!(txt.contains("dois"));
        assert!(out.contains("- [H ] um"));
        assert!(out.contains("- [H x] dois"));
        assert!(mark_human_str(s, None, Some(9)).is_err(), "índice fora de faixa → erro");
    }

    #[test]
    fn note_block_rotula_por_tipo() {
        assert!(note_block("correcao", "arrume X").contains("PROMPT DE CORREÇÃO"));
        assert!(note_block("task", "detalhe Y").contains("PONTO POR TASK"));
        assert!(note_block("correcao", "arrume X").contains("arrume X"));
    }

    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// Cria um root de projeto temporário com `.overdev/CHECKLIST.md` = `body`.
    fn fresh_root(body: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("schematize-comp-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".overdev")).unwrap();
        fs::write(root.join(".overdev").join("CHECKLIST.md"), body).unwrap();
        root
    }

    #[test]
    fn done_item_text_ignora_humano_e_abertos() {
        assert_eq!(done_item_text("- [x] feito A").as_deref(), Some("feito A"));
        assert_eq!(done_item_text("  - [X] indentado").as_deref(), Some("indentado"));
        assert_eq!(done_item_text("- [H x] humano feito"), None, "humano NÃO é máquina");
        assert_eq!(done_item_text("- [ ] aberto"), None);
        assert_eq!(done_item_text("- [~] on-hold"), None);
    }

    #[test]
    fn record_seed_depois_detecta_novo() {
        // SEED: 1ª chamada popula os `- [x]` já presentes (com o mtime do CHECKLIST).
        let root = fresh_root("- [x] item feito antes\n- [ ] ainda aberto\n");
        let seeded = record_completions(&root);
        assert_eq!(seeded.len(), 1, "só o `- [x]` presente entra no seed");
        assert_eq!(seeded[0].text, "item feito antes");
        let seed_ts = seeded[0].ts;
        assert!(seed_ts > 0, "seed usa o mtime do CHECKLIST (>0)");
        assert!(completions_file(&root).exists(), "1ª chamada grava o registro");

        // Agora o Claude fecha o item aberto POR FORA: vira `- [x]`.
        fs::write(
            root.join(".overdev").join("CHECKLIST.md"),
            "- [x] item feito antes\n- [x] agora fechado\n",
        )
        .unwrap();
        let after = record_completions(&root);
        assert_eq!(after.len(), 2, "detecta o novo `- [x]` por diff");
        let find = |t: &str| after.iter().find(|c| c.text == t).map(|c| c.ts);
        let seed_now = find("item feito antes").expect("seed presente");
        let novo = find("agora fechado").expect("novo item detectado");
        // Está ordenado por ts asc (com desempate por nome).
        assert!(after.windows(2).all(|w| w[0].ts <= w[1].ts), "ordenado por ts asc");
        // O seed NÃO é reescrito (mantém o ts do mtime original).
        assert_eq!(seed_now, seed_ts);
        // O novo item nunca é anterior ao seed.
        assert!(novo >= seed_now, "novo item tem ts >= o do seed");
    }

    #[test]
    fn completions_so_le_sem_gravar() {
        let root = fresh_root("- [x] a\n");
        // Sem registro ainda: completions() lê vazio e NÃO cria o arquivo.
        assert!(completions(&root).is_empty());
        assert!(!completions_file(&root).exists(), "completions() não grava");
        // Depois de record_completions, completions() lê o mesmo conteúdo.
        let rec = record_completions(&root);
        let read = completions(&root);
        assert_eq!(rec, read);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].text, "a");
    }
}

#[cfg(test)]
mod archive_name_tests {
    use super::*;
    #[test]
    fn projeto_e_prefixo_comum_dos_microservicos() {
        let tmp = std::env::temp_dir().join(format!("schz-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for ms in ["schematize_cli_rs","schematizeskills_api_rs","schematize_gui_slint"] {
            std::fs::create_dir_all(tmp.join(ms)).unwrap();
        }
        assert_eq!(project_name(&tmp), "schematize");
        assert_eq!(archive_dir(&tmp).unwrap(), tmp.join("schematize_archive"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
