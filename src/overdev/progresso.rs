//! PROGRESSO do run: o checklist 2-níveis contado (máquina/humano/on-hold) e os
//! itens abertos. É daqui que sai o número que a GUI e o gate leem.

use super::*;

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
    /// Recusados/cancelados (`- [H -]`, `- [-]`): resolvidos e NÃO feitos.
    pub recusado: usize,
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
    let c = count_str(&crate::paths::read_multidoc(&d, "CHECKLIST.md", "checklist"));
    let it = fs::read_to_string(d.join("iterations"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    Progress {
        mode: st.as_ref().map(|s| s.mode.clone()).unwrap_or_default(),
        open: c.open,
        done: c.done(),
        hold: c.hold,
        human: c.human,
        recusado: c.recusado,
        iterations: it,
        max_iters: st.as_ref().map(|s| s.max_iters).unwrap_or(0),
    }
}

/// Itens de MÁQUINA abertos (`- [ ]`) do CHECKLIST de `<root>` (até `limit`).
/// É a lista que o auto-continue reenvia ao agente ocioso.
pub fn open_items_at(root: &Path, limit: usize) -> Vec<String> {
    items_with_marker(
        &crate::paths::read_multidoc(&dir_at(root), "CHECKLIST.md", "checklist"),
        "- [ ]",
        limit,
    )
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
    txt.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).map(String::from)
}

/// Checklist de 2 níveis: contagem das 4 categorias.
/// `open` = máquina-abertos (`- [ ]`); `human` = humano-abertos (`- [H ]`);
/// `hold` = on-hold (`- [~]`); feitos separados por origem (`- [x]` vs `- [H x]`).
#[derive(Default, PartialEq, Eq, Debug)]
pub(crate) struct Counts {
    pub(crate) open: usize,   // - [ ]  (máquina, o agente fecha)
    pub(crate) done_m: usize, // - [x]  (máquina feito)
    pub(crate) done_h: usize, // - [H x] (humano feito)
    pub(crate) hold: usize,   // - [~]  (on-hold, pergunta parkeada)
    pub(crate) human: usize,  // - [H ] (humano aberto, só a pessoa fecha)
    /// `- [H r]` — a pessoa RESPONDEU (decidiu; não executou). Fecha o item humano e,
    /// se havia vínculo, o item de máquina volta pra `- [ ]` sozinho.
    pub(crate) respondido: usize,
    /// `- [H -]` e `- [-]` — recusados/cancelados. Resolvidos, mas NÃO feitos.
    ///
    /// Conta à parte de propósito. Somar em `done` inflaria o progresso com trabalho
    /// que ninguém fez; deixar em `open` travaria o overdev pra sempre, porque o gate
    /// se recusa a parar enquanto houver item de máquina aberto. Nenhuma das duas
    /// gavetas existentes servia — por isso a terceira.
    pub(crate) recusado: usize,
}

impl Counts {
    /// Feitos totais (contrato do check): máquina + humano.
    pub(crate) fn done(&self) -> usize {
        // Respondido conta como resolvido: a pendência humana acabou. Recusado NÃO —
        // ver a nota no campo.
        self.done_m + self.done_h + self.respondido
    }
}

/// Reconhece os marcadores de 2 níveis. IMPORTANTE: casar `- [H ...]` ANTES de
/// `- [ ]`/`- [x]` pra o item humano não cair no ramo de máquina.
pub(crate) fn count_str(s: &str) -> Counts {
    let mut c = Counts::default();
    for l in s.lines() {
        let t = l.trim_start();
        if t.starts_with("- [H ]") {
            c.human += 1;
        } else if t.starts_with("- [H x]") || t.starts_with("- [H X]") {
            c.done_h += 1;
        } else if t.starts_with("- [H r]") || t.starts_with("- [H R]") {
            c.respondido += 1;
        } else if t.starts_with("- [H -]") || t.starts_with("- [-]") {
            c.recusado += 1;
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

pub(crate) fn counts() -> Counts {
    count_str(&crate::paths::read_multidoc(&dir(), "CHECKLIST.md", "checklist"))
}

/// Linhas cujo `trim_start` começa por `marker` (ex.: "- [ ]" ou "- [H ]").
pub(crate) fn items_with_marker(s: &str, marker: &str, limit: usize) -> Vec<String> {
    s.lines()
        .filter(|l| l.trim_start().starts_with(marker))
        .take(limit)
        .map(|l| l.trim().replace('"', "'"))
        .collect()
}

pub(crate) fn open_items() -> Vec<String> {
    items_with_marker(&crate::paths::read_multidoc(&dir(), "CHECKLIST.md", "checklist"), "- [ ]", 8)
}

pub(crate) fn human_items() -> Vec<String> {
    items_with_marker(
        &crate::paths::read_multidoc(&dir(), "CHECKLIST.md", "checklist"),
        "- [H ]",
        12,
    )
}
