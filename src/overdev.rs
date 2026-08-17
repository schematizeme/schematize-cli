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

const DEFAULT_MAX_ITERS: u64 = 200;
const QUESTIONS_FILE: &str = "PERGUNTAS-OVERDEV.txt";

#[derive(Serialize, Deserialize)]
struct OverState {
    mode: String, // active | done | blocked | stopped
    max_iters: u64,
    objetivo: String,
    started: u64,
}

fn dir() -> PathBuf {
    PathBuf::from(".overdev")
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

/// `<root>/.overdev`.
fn dir_at(root: &Path) -> PathBuf {
    root.join(".overdev")
}

/// Carrega o state.json de um projeto arbitrário (None se ausente/ilegível).
fn load_at(root: &Path) -> Option<OverState> {
    fs::read_to_string(dir_at(root).join("state.json")).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Snapshot do progresso de um run (peça pública, path-aware, sem efeito colateral).
/// Consumido pelo monitor do `run_attached` e pela GUI (barra de progresso + contagem).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    /// `active` | `stopped` | `done` | `blocked`; vazio se não há run.
    pub mode: String,
    /// Itens de MÁQUINA abertos (`- [ ]`) — o que trava a conclusão.
    pub open: usize,
    /// Feitos totais (máquina `- [x]` + humano `- [H x]`).
    pub done: usize,
    /// On-hold (`- [~]`, pergunta parkeada).
    pub hold: usize,
    /// Humanos abertos (`- [H ]`) — só a pessoa fecha.
    pub human: usize,
    /// Ciclos já gastos (arquivo `iterations`).
    pub iterations: u64,
    /// Teto de ciclos do run.
    pub max_iters: u64,
}

impl Progress {
    /// `true` quando não há mais trabalho de MÁQUINA nem o run está parado —
    /// critério de término do `run_attached` junto com `mode == "stopped"`.
    pub fn finished(&self) -> bool {
        self.mode == "stopped" || (self.mode == "active" && self.open == 0)
    }
}

/// Lê o progresso do run em `<root>/.overdev` (state.json + CHECKLIST.md + iterations).
pub fn progress_at(root: &Path) -> Progress {
    let d = dir_at(root);
    let st = load_at(root);
    let c = count_str(&fs::read_to_string(d.join("CHECKLIST.md")).unwrap_or_default());
    let it = fs::read_to_string(d.join("iterations")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    Progress {
        mode: st.as_ref().map(|s| s.mode.clone()).unwrap_or_default(),
        open: c.open,
        done: c.done(),
        hold: c.hold,
        human: c.human,
        iterations: it,
        max_iters: st.as_ref().map(|s| s.max_iters).unwrap_or(0),
    }
}

/// Itens de MÁQUINA abertos (`- [ ]`) do CHECKLIST de `<root>` (até `limit`).
/// É a lista que o auto-continue reenvia ao agente ocioso.
pub fn open_items_at(root: &Path, limit: usize) -> Vec<String> {
    items_with_marker(&fs::read_to_string(dir_at(root).join("CHECKLIST.md")).unwrap_or_default(), "- [ ]", limit)
}

/// Objetivo do run pra montar o prompt inicial do agente: prioriza o campo
/// `objetivo` do state.json; se vazio, a 1ª linha útil de `.overdev/OBJETIVO.md`
/// (ignora linhas em branco e comentários `#`). None se nada disso existir.
pub fn objetivo_at(root: &Path) -> Option<String> {
    if let Some(st) = load_at(root) {
        let o = st.objetivo.trim();
        if !o.is_empty() {
            return Some(o.to_string());
        }
    }
    let txt = fs::read_to_string(dir_at(root).join("OBJETIVO.md")).ok()?;
    txt.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
}

// ---------------------------------------------------------------------------
// LOG DE CONCLUSÕES: cada `- [x]` (máquina) que aparece no CHECKLIST ganha a HORA
// em que foi detectado. Como o Claude edita o CHECKLIST direto (às vezes POR FORA
// do app), a detecção é por DIFF e é acionada no check() (Stop hook, a cada turno).
// Registro em `.overdev/completions.json` (mapa text->ts, epoch secs).
// ---------------------------------------------------------------------------

/// Uma conclusão de item de MÁQUINA (`- [x]`) com a hora em que foi detectada.
/// `text` = conteúdo do item sem o prefixo `- [x] `; `ts` = epoch secs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub text: String,
    pub ts: i64,
}

fn completions_file(root: &Path) -> PathBuf {
    dir_at(root).join("completions.json")
}

/// Epoch (secs) agora, como i64.
fn now_secs_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// mtime (epoch secs) de um arquivo; None se indisponível.
fn mtime_secs(p: &Path) -> Option<i64> {
    let m = fs::metadata(p).ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
}

/// Texto de um item `- [x]` (máquina feito), sem o prefixo. None se a linha não
/// for `- [x]`. NÃO casa `- [H x]` (humano): esse começa por `- [H`, não `- [x]`.
fn done_item_text(line: &str) -> Option<String> {
    let t = line.trim_start();
    if t.starts_with("- [x]") || t.starts_with("- [X]") {
        Some(t[5..].trim().to_string())
    } else {
        None
    }
}

/// Ordena e materializa o mapa text->ts em `Vec<Completion>` (ts asc, nome desempata).
fn sorted_completions(map: std::collections::BTreeMap<String, i64>) -> Vec<Completion> {
    let mut out: Vec<Completion> = map.into_iter().map(|(text, ts)| Completion { text, ts }).collect();
    out.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.text.cmp(&b.text)));
    out
}

/// Lê o registro `.overdev/completions.json` (mapa text->ts). Vazio se ausente/ilegível.
fn read_completions_map(root: &Path) -> std::collections::BTreeMap<String, i64> {
    fs::read_to_string(completions_file(root)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Detecta e REGISTRA (por DIFF) as conclusões de máquina do CHECKLIST de `<root>`.
/// - Lê os `- [x]` do CHECKLIST e o registro `.overdev/completions.json`.
/// - Para cada `- [x]` NOVO (fora do mapa) adiciona com `now`.
/// - SEED: se o registro ainda não existe, popula os `- [x]` JÁ presentes com o
///   mtime do CHECKLIST.md (aproxima "feito antes do log começar" em vez de perder).
/// - Salva o JSON e retorna TODAS as conclusões ordenadas por `ts` asc.
/// Best-effort: qualquer erro de IO é ignorado (nunca panica).
pub fn record_completions(root: &Path) -> Vec<Completion> {
    let checklist_path = dir_at(root).join("CHECKLIST.md");
    let content = match fs::read_to_string(&checklist_path) {
        Ok(c) => c,
        Err(_) => return sorted_completions(read_completions_map(root)),
    };
    let done_texts: Vec<String> = content.lines().filter_map(done_item_text).collect();

    let cf = completions_file(root);
    let existed = cf.exists();
    let mut map = read_completions_map(root);

    // 1ª vez (sem registro): os `- [x]` já presentes recebem o mtime do CHECKLIST.
    let seed_ts = if existed { 0 } else { mtime_secs(&checklist_path).unwrap_or_else(now_secs_i64) };
    let now = now_secs_i64();

    let mut changed = false;
    for txt in done_texts {
        if !map.contains_key(&txt) {
            map.insert(txt, if existed { now } else { seed_ts });
            changed = true;
        }
    }
    // Grava sempre na 1ª vez (marca que o seed já ocorreu) ou quando houver item novo.
    if changed || !existed {
        if let Some(d) = cf.parent() {
            let _ = fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(&map) {
            let _ = fs::write(&cf, s);
        }
    }
    sorted_completions(map)
}

/// Só LÊ o `.overdev/completions.json` (sem gravar), ordenado por `ts` asc —
/// pra a GUI/poll ler barato, sem tocar no CHECKLIST.
pub fn completions(root: &Path) -> Vec<Completion> {
    sorted_completions(read_completions_map(root))
}

fn save(st: &OverState) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    fs::write(state_file(), serde_json::to_string_pretty(st).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// Checklist de 2 níveis: contagem das 4 categorias.
/// `open` = máquina-abertos (`- [ ]`); `human` = humano-abertos (`- [H ]`);
/// `hold` = on-hold (`- [~]`); feitos separados por origem (`- [x]` vs `- [H x]`).
#[derive(Default, PartialEq, Eq, Debug)]
struct Counts {
    open: usize,   // - [ ]  (máquina, o agente fecha)
    done_m: usize, // - [x]  (máquina feito)
    done_h: usize, // - [H x] (humano feito)
    hold: usize,   // - [~]  (on-hold, pergunta parkeada)
    human: usize,  // - [H ] (humano aberto, só a pessoa fecha)
}
impl Counts {
    /// Feitos totais (contrato do check): máquina + humano.
    fn done(&self) -> usize {
        self.done_m + self.done_h
    }
}

/// Reconhece os marcadores de 2 níveis. IMPORTANTE: casar `- [H ...]` ANTES de
/// `- [ ]`/`- [x]` pra o item humano não cair no ramo de máquina.
fn count_str(s: &str) -> Counts {
    let mut c = Counts::default();
    for l in s.lines() {
        let t = l.trim_start();
        if t.starts_with("- [H ]") {
            c.human += 1;
        } else if t.starts_with("- [H x]") || t.starts_with("- [H X]") {
            c.done_h += 1;
        } else if t.starts_with("- [ ]") {
            c.open += 1;
        } else if t.starts_with("- [x]") || t.starts_with("- [X]") {
            c.done_m += 1;
        } else if t.starts_with("- [~]") {
            c.hold += 1;
        }
    }
    c
}

fn counts() -> Counts {
    count_str(&fs::read_to_string(checklist()).unwrap_or_default())
}

/// Linhas cujo `trim_start` começa por `marker` (ex.: "- [ ]" ou "- [H ]").
fn items_with_marker(s: &str, marker: &str, limit: usize) -> Vec<String> {
    s.lines()
        .filter(|l| l.trim_start().starts_with(marker))
        .take(limit)
        .map(|l| l.trim().replace('"', "'"))
        .collect()
}

fn open_items() -> Vec<String> {
    items_with_marker(&fs::read_to_string(checklist()).unwrap_or_default(), "- [ ]", 8)
}

fn human_items() -> Vec<String> {
    items_with_marker(&fs::read_to_string(checklist()).unwrap_or_default(), "- [H ]", 12)
}

/// Roda o gate opcional (.overdev/gate.sh). true se não há gate ou se passa.
fn gate_ok() -> bool {
    let g = dir().join("gate.sh");
    if !g.exists() {
        return true;
    }
    util::run("bash", &[g.to_str().unwrap()]).is_ok()
}

fn drain_stdin() {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
}

fn print_block(reason: &str) {
    let v = serde_json::json!({ "decision": "block", "reason": reason });
    println!("{v}");
}

/// Inicia um run de overdev no diretório atual.
pub fn start(objetivo: &str, max_iters: Option<u64>) -> Result<(), String> {
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
    // Garante gitignore do control-plane.
    let gi = Path::new(".gitignore");
    let cur = fs::read_to_string(gi).unwrap_or_default();
    if !cur.contains(".overdev") {
        let _ = fs::write(gi, format!("{cur}\n.overdev/\n"));
    }
    println!("overdev ATIVO. Objetivo: {objetivo}");
    println!("Preencha .overdev/CHECKLIST.md (exaustivo). O agente não pode parar até fechar.");
    // Snapshot inicial no DB local (best-effort: nunca quebra o start se o DB falhar).
    let _ = crate::overdevdb::snapshot(Path::new("."));
    Ok(())
}

/// Comando que a GUI injeta na sessão do agente acoplado pra CARREGAR os preceitos
/// de engenharia da casa no contexto (slash command da skill de engenharia).
pub fn load_cmd() -> &'static str {
    "/eng-load"
}

/// Comando que a GUI injeta pra (re)INDEXAR o conteúdo do projeto (grafo/MAPA §39).
pub fn index_cmd() -> &'static str {
    "/eng-index"
}

/// Veredito do Stop hook, decidido de forma PURA (testável) a partir das contagens.
/// "Concluído de verdade" = zero `- [ ]` E zero `- [H ]`. On-hold nunca bloqueia.
#[derive(PartialEq, Eq, Debug)]
enum StopDecision {
    /// Parada legítima e silenciosa (nada aberto, gate verde).
    Permit,
    /// Máquina terminou o dela, mas restam itens HUMANOS. Permite parar,
    /// porém informa (não bloqueia) — o humano fecha pela GUI/CLI.
    PermitWithHuman(String),
    /// Rejeita a parada com a mensagem de bloqueio.
    Block(String),
}

/// Regra do check: enquanto houver `- [ ]` (máquina) → BLOCK. Se não houver `- [ ]`
/// mas houver `- [H ]` (humano) → PERMITE parar e lista os pendentes humanos.
/// Gate falhando com tudo de máquina feito também BLOCK (a máquina conserta).
fn decide_stop(
    c: &Counts,
    gate_ok: bool,
    open_items: &[String],
    human_items: &[String],
    it: u64,
    max: u64,
) -> StopDecision {
    if c.open > 0 {
        let items = open_items.join("\\n");
        return StopDecision::Block(format!(
            "MODO OVERDEV — NÃO PARE E NÃO DIGA QUE TERMINOU. Faltam {} item(ns) de MÁQUINA abertos ({} on-hold · {} humanos, ciclo {it}/{max}). NÃO fale com o usuário ainda. Pegue o PRÓXIMO '- [ ]', implemente, VERIFIQUE (gate/teste), marque '- [x]'. Se travar por dúvida: NÃO use AskUserQuestion — escreva a pergunta em ./{QUESTIONS_FILE}, marque o item '- [~]' (on-hold) e SIGA. Itens de máquina abertos:\\n{items}\\nOs '- [H ]' (humanos) NÃO são seus — deixe pro humano. Só pare quando não sobrar '- [ ]' e o gate passar.",
            c.open, c.hold, c.human
        ));
    }
    // Sem trabalho de máquina em aberto. Gate ainda manda na máquina.
    if !gate_ok {
        return StopDecision::Block(format!(
            "MODO OVERDEV — NÃO PARE. O checklist de máquina está todo marcado mas o gate (.overdev/gate.sh) FALHOU (ciclo {it}/{max}). Conserte o que o gate acusa e rode de novo. Só pare com o gate verde."
        ));
    }
    if c.human > 0 {
        let items = human_items.join("\n");
        return StopDecision::PermitWithHuman(format!(
            "MODO OVERDEV — a parte da MÁQUINA está concluída (gate verde). Faltam {} item(ns) HUMANOS — feche pela GUI ou por `schematize overdev human \"<texto>\"`:\n{}",
            c.human, items
        ));
    }
    StopDecision::Permit
}

/// Stop hook: rejeita a parada enquanto houver `- [ ]` (trabalho de máquina).
/// Quando só restam `- [H ]` (humanos), PERMITE parar e informa os pendentes.
pub fn check() {
    drain_stdin();
    let st = match load() {
        Some(s) if s.mode == "active" => s,
        _ => return, // inerte
    };
    // LOG de conclusões (best-effort): detecta os `- [x]` novos por DIFF e registra
    // a hora. Fica na trilha do Stop hook, então também pega run EXTERNO (fora do app).
    // Não altera a decisão de parar.
    let _ = record_completions(Path::new("."));
    // budget
    let mut it: u64 = fs::read_to_string(iters_file()).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    it += 1;
    let _ = fs::write(iters_file(), it.to_string());
    if it > st.max_iters {
        return; // guardrail anti-loop: permite parar
    }
    let c = counts();
    match decide_stop(&c, gate_ok(), &open_items(), &human_items(), it, st.max_iters) {
        StopDecision::Permit => {}
        // Permite a parada (não bloqueia), mas mostra os itens humanos pendentes.
        StopDecision::PermitWithHuman(msg) => println!("{msg}"),
        StopDecision::Block(reason) => print_block(&reason),
    }
}

/// PreToolUse hook (matcher AskUserQuestion): veta a pergunta bloqueante em overdev.
pub fn guard() {
    drain_stdin();
    let active = matches!(load(), Some(s) if s.mode == "active");
    if !active {
        return; // fora de overdev: libera normalmente
    }
    let reason = format!(
        "VETADO em OVERDEV: nada de parar pra perguntar com pool bloqueante. Escreva a pergunta em ./{QUESTIONS_FILE} (na base do projeto), marque o item correspondente como '- [~]' (on-hold) no .overdev/CHECKLIST.md com `schematize overdev hold`, e CONTINUE os outros itens. As perguntas serão respondidas quando o usuário voltar."
    );
    let v = serde_json::json!({
        "decision": "block",
        "reason": reason,
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        }
    });
    println!("{v}");
}

/// Parkeia uma pergunta: registra no txt da base e marca o item como on-hold.
pub fn park(item_substr: &str, pergunta: &str) -> Result<(), String> {
    // 1) registra a pergunta na base do projeto.
    let mut q = fs::read_to_string(QUESTIONS_FILE).unwrap_or_default();
    q.push_str(&format!("[{}] item: {item_substr}\n  pergunta: {pergunta}\n\n", util::now_unix()));
    fs::write(QUESTIONS_FILE, q).map_err(|e| e.to_string())?;
    // 2) marca o primeiro item aberto que casa como on-hold.
    hold(item_substr)?;
    println!("pergunta parkeada em ./{QUESTIONS_FILE}; item marcado on-hold. Siga os demais.");
    Ok(())
}

/// Marca o primeiro `- [ ]` que contém `substr` como `- [~]` (on-hold).
pub fn hold(substr: &str) -> Result<(), String> {
    let s = fs::read_to_string(checklist()).map_err(|e| e.to_string())?;
    let mut done = false;
    let out: Vec<String> = s
        .lines()
        .map(|l| {
            if !done && l.trim_start().starts_with("- [ ]") && l.contains(substr) {
                done = true;
                l.replacen("- [ ]", "- [~]", 1)
            } else {
                l.to_string()
            }
        })
        .collect();
    fs::write(checklist(), out.join("\n")).map_err(|e| e.to_string())?;
    if !done {
        return Err(format!("nenhum item aberto contém '{substr}'"));
    }
    Ok(())
}

/// Fecha o primeiro `- [H ]` (humano aberto) → `- [H x]` — PURO, testável.
/// Casa por `substr` (contém) OU por `index` (1-based entre os humanos abertos).
/// Retorna (novo conteúdo, texto do item fechado).
fn mark_human_str(s: &str, substr: Option<&str>, index: Option<usize>) -> Result<(String, String), String> {
    let mut seen = 0usize; // contador de humanos abertos vistos (1-based)
    let mut hit: Option<String> = None;
    let out: Vec<String> = s
        .lines()
        .map(|l| {
            if hit.is_none() && l.trim_start().starts_with("- [H ]") {
                seen += 1;
                let matches = match (substr, index) {
                    (_, Some(n)) => seen == n,
                    (Some(sub), None) => l.contains(sub),
                    (None, None) => false,
                };
                if matches {
                    hit = Some(l.trim().to_string());
                    return l.replacen("- [H ]", "- [H x]", 1);
                }
            }
            l.to_string()
        })
        .collect();
    match hit {
        Some(txt) => Ok((out.join("\n"), txt)),
        None => Err(match (substr, index) {
            (_, Some(n)) => format!("não há {n}º item humano aberto (- [H ])"),
            (Some(sub), None) => format!("nenhum item humano aberto contém '{sub}'"),
            (None, None) => "informe o texto do item ou --done <n>".to_string(),
        }),
    }
}

/// CLI: o HUMANO fecha um item `- [H ]` → `- [H x]` (pela CLI ou GUI).
/// `substr` casa pelo texto; `index` (--done N) casa pela posição entre os humanos abertos.
pub fn human_done(substr: Option<&str>, index: Option<usize>) -> Result<(), String> {
    let s = fs::read_to_string(checklist()).map_err(|e| e.to_string())?;
    let (out, txt) = mark_human_str(&s, substr, index)?;
    fs::write(checklist(), out).map_err(|e| e.to_string())?;
    // Versiona a mudança no DB local (best-effort).
    let _ = crate::overdevdb::snapshot(Path::new("."));
    println!("item humano fechado: {txt}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Notas do humano: prompt de correção do overdev + pontos por task.
// Gravadas em .overdev/NOTAS.md pra a GUI ler depois. `add_note`/`read_notes`.
// ---------------------------------------------------------------------------

fn notas_file(root: &Path) -> PathBuf {
    root.join(".overdev").join("NOTAS.md")
}

/// Formata um bloco de nota (PURO). `kind`: "correcao" (prompt de correção do
/// overdev), "task" (ponto específico por task) ou livre; `texto` é o conteúdo.
fn note_block(kind: &str, texto: &str) -> String {
    let label = match kind {
        "correcao" | "correction" => "PROMPT DE CORREÇÃO",
        "task" => "PONTO POR TASK",
        other => other,
    };
    format!("## [{}] {}\n\n{}\n\n", util::now_unix(), label, texto.trim())
}

/// Anexa uma nota do humano em `<root>/.overdev/NOTAS.md` (cria se preciso).
pub fn add_note(root: &Path, kind: &str, texto: &str) -> Result<(), String> {
    let f = notas_file(root);
    if let Some(d) = f.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let mut cur = fs::read_to_string(&f).unwrap_or_else(|_| "# OVERDEV — NOTAS do humano\n\n".to_string());
    cur.push_str(&note_block(kind, texto));
    fs::write(&f, cur).map_err(|e| e.to_string())?;
    // Versiona a nota no DB local (best-effort).
    let _ = crate::overdevdb::snapshot(root);
    Ok(())
}

/// Lê as notas do humano (vazio se não houver) — consumível pela GUI.
pub fn read_notes(root: &Path) -> String {
    fs::read_to_string(notas_file(root)).unwrap_or_default()
}

/// CLI: `schematize overdev note "<texto>" [--kind correcao|task]`.
pub fn note(kind: &str, texto: &str) -> Result<(), String> {
    add_note(Path::new("."), kind, texto)?;
    println!("nota registrada em .overdev/NOTAS.md ({kind}).");
    Ok(())
}

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
                println!("notas do humano: {nn} (ver .overdev/NOTAS.md)");
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
