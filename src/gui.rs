//! schematize-gui — janela de gerenciamento (egui/eframe 0.36). O quê: gestor de skills
//! — marca EXATAMENTE o que quer e instala/atualiza/remove em MASSA e em PARALELO
//! (checkboxes + ações em lote), troca de idioma, liga agente/overdev, abre painel/site.
//! Onde: binário separado (feature `gui`); usa a lib `schematize`. Precisa de X11/Wayland/GL.
// No Windows (release), roda como app gráfico — sem janela de console atrás.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui;
use eframe::egui::IconData;
use schematize::i18n::{self, t, tf};
use schematize::registry::Item;
use schematize::{links, registry, selfupdate, skills, util};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Uma linha da tabela: uma skill (com `item`) ou o próprio CLI (`item` None).
#[derive(Clone)]
struct Row {
    name: String,
    installed: Option<String>,
    latest: Option<String>,
    item: Option<Item>, // None = CLI (atualiza por self-update, não é removível)
}

impl Row {
    fn outdated(&self) -> bool {
        matches!((&self.installed, &self.latest), (Some(i), Some(l)) if i != l)
    }
    fn missing(&self) -> bool {
        self.installed.is_none()
    }
    fn pending(&self) -> bool {
        self.outdated() || self.missing()
    }
}

/// Uma operação de lote a rodar em paralelo.
enum Op {
    Install(Item), // instalar/atualizar uma skill (idempotente = baixa o latest)
    Remove(Item),  // desinstalar uma skill
    SelfUpdate,    // atualizar o próprio CLI/GUI (sem sudo)
}

/// Coleta o estado atual de todas as linhas (skills + CLI). Roda em thread.
/// As skills vêm do ÍNDICE REMOTO (registry::catalog) — skills novas aparecem sozinhas.
fn collect_rows() -> Vec<Row> {
    let mut rows: Vec<Row> = registry::catalog()
        .into_iter()
        .map(|it| Row {
            name: it.slug.clone(),
            installed: skills::installed_version(&it),
            latest: skills::resolve_latest(&it).ok(),
            item: Some(it),
        })
        .collect();
    rows.push(Row {
        name: "schematize (CLI)".into(),
        installed: Some(env!("CARGO_PKG_VERSION").to_string()),
        latest: skills::latest_release_tag("schematize-cli"),
        item: None,
    });
    rows
}

/// Texto de status a partir das linhas (localizado).
fn status_for(rows: &[Row]) -> String {
    let n = rows.iter().filter(|r| r.pending()).count();
    if n == 0 {
        t("gui.all_uptodate")
    } else {
        tf("gui.n_pending", &[("n", &n.to_string())])
    }
}

/// Estado localizado de uma linha para a coluna "estado".
fn state_text(r: &Row) -> String {
    if r.missing() {
        t("common.not_installed")
    } else if r.outdated() {
        t("common.update")
    } else {
        t("common.current")
    }
}

struct App {
    rows: Vec<Row>,
    selected: HashSet<String>, // chave = Row.name
    status: String,
    lang: String,
    busy: Arc<AtomicBool>,
    tx: Sender<(Vec<Row>, String)>,
    rx: Receiver<(Vec<Row>, String)>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (tx, rx) = channel();
        let app = Self {
            rows: vec![],
            selected: HashSet::new(),
            status: t("gui.loading"),
            lang: i18n::current_code(),
            busy: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        };
        app.refresh(cc.egui_ctx.clone());
        app
    }

    /// Dispara a coleta (instalada no disco + latest via API) numa thread.
    fn refresh(&self, ctx: egui::Context) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let rows = collect_rows();
            let st = status_for(&rows);
            let _ = tx.send((rows, st));
            ctx.request_repaint();
        });
    }

    /// Roda um LOTE de operações em PARALELO numa thread coordenadora (uma só trava de busy),
    /// depois recoleta o estado. É o coração do "instalar/atualizar/remover em massa".
    fn run_batch(&self, ops: Vec<Op>, ctx: egui::Context) {
        if ops.is_empty() || self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // I/O-bound (curl/unzip): roda todas as ops concorrentes e junta.
            std::thread::scope(|sc| {
                for op in &ops {
                    sc.spawn(move || match op {
                        Op::Install(it) => {
                            let _ = skills::install(it);
                        }
                        Op::Remove(it) => {
                            let _ = skills::remove(it);
                        }
                        Op::SelfUpdate => {
                            let _ = selfupdate::run();
                        }
                    });
                }
            });
            let rows = collect_rows();
            let st = status_for(&rows);
            let _ = tx.send((rows, st));
            ctx.request_repaint();
        });
    }

    /// Op de instalar/atualizar para uma linha (skill → Install; CLI → SelfUpdate).
    fn apply_op(r: &Row) -> Op {
        match &r.item {
            Some(it) => Op::Install(it.clone()),
            None => Op::SelfUpdate,
        }
    }

    /// Ops para instalar/atualizar TODAS as pendentes (skills faltando/desatualizadas + CLI).
    fn ops_all_pending(&self) -> Vec<Op> {
        self.rows.iter().filter(|r| r.pending()).map(Self::apply_op).collect()
    }

    /// Ops de instalar/atualizar a partir da SELEÇÃO (só as que fazem sentido: pendentes).
    fn ops_apply_selected(&self) -> Vec<Op> {
        self.rows
            .iter()
            .filter(|r| self.selected.contains(&r.name) && r.pending())
            .map(Self::apply_op)
            .collect()
    }

    /// Ops de remover a partir da SELEÇÃO (só skills instaladas; CLI nunca é removido).
    fn ops_remove_selected(&self) -> Vec<Op> {
        self.rows
            .iter()
            .filter(|r| self.selected.contains(&r.name) && !r.missing())
            .filter_map(|r| r.item.clone().map(Op::Remove))
            .collect()
    }

    fn count_apply_selected(&self) -> usize {
        self.rows.iter().filter(|r| self.selected.contains(&r.name) && r.pending()).count()
    }
    fn count_remove_selected(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| self.selected.contains(&r.name) && !r.missing() && r.item.is_some())
            .count()
    }
}

fn shell(cmd: &str) -> String {
    match util::run("bash", &["-lc", cmd]) {
        Ok(o) => o.trim().to_string(),
        Err(e) => format!("erro: {e}"),
    }
}

impl eframe::App for App {
    // eframe 0.36: o ponto de entrada é `ui` (recebe um Ui raiz), não mais `update`.
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        while let Ok((rows, st)) = self.rx.try_recv() {
            self.rows = rows;
            self.status = st;
            self.busy.store(false, Ordering::SeqCst);
            // remove da seleção nomes que não existem mais (skills removidas)
            let names: HashSet<String> = self.rows.iter().map(|r| r.name.clone()).collect();
            self.selected.retain(|n| names.contains(n));
        }
        let busy = self.busy.load(Ordering::SeqCst);
        let ctx = ui.ctx().clone();

        egui::Panel::top("top").resizable(false).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("schematize");
                ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());

                // seletor de idioma
                let mut sel = self.lang.clone();
                egui::ComboBox::from_id_salt("lang")
                    .selected_text(i18n::name_of(&sel).unwrap_or("English"))
                    .show_ui(ui, |ui| {
                        for (c, name, _) in i18n::LANGS {
                            ui.selectable_value(&mut sel, (*c).to_string(), *name);
                        }
                    });
                if sel != self.lang {
                    let _ = i18n::set_lang(&sel);
                    self.lang = sel;
                    self.status = status_for(&self.rows);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!busy, egui::Button::new(t("gui.check"))).clicked() {
                        self.refresh(ctx.clone());
                    }
                    let has_pending = self.rows.iter().any(|r| r.pending());
                    if ui.add_enabled(!busy && has_pending, egui::Button::new(t("gui.update_all"))).clicked() {
                        self.run_batch(self.ops_all_pending(), ctx.clone());
                    }
                });
            });

            // barra de seleção em massa (o "gestor de verdade")
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t("gui.sel_label"));
                if ui.add_enabled(!busy, egui::Button::new(t("gui.sel_all"))).clicked() {
                    self.selected = self.rows.iter().map(|r| r.name.clone()).collect();
                }
                if ui.add_enabled(!busy, egui::Button::new(t("gui.sel_pending"))).clicked() {
                    self.selected = self.rows.iter().filter(|r| r.pending()).map(|r| r.name.clone()).collect();
                }
                if ui.add_enabled(!busy, egui::Button::new(t("gui.sel_none"))).clicked() {
                    self.selected.clear();
                }

                ui.separator();
                let n_apply = self.count_apply_selected();
                let n_rm = self.count_remove_selected();
                if ui
                    .add_enabled(!busy && n_apply > 0, egui::Button::new(format!("{} ({n_apply})", t("gui.install_sel"))))
                    .clicked()
                {
                    let ops = self.ops_apply_selected();
                    self.run_batch(ops, ctx.clone());
                }
                if ui
                    .add_enabled(!busy && n_rm > 0, egui::Button::new(format!("{} ({n_rm})", t("gui.remove_sel"))))
                    .clicked()
                {
                    let ops = self.ops_remove_selected();
                    self.run_batch(ops, ctx.clone());
                }
            });

            ui.add_space(4.0);
            ui.label(if busy { t("gui.working") } else { self.status.clone() });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("skills").num_columns(5).striped(true).spacing([14.0, 8.0]).show(ui, |ui| {
                    ui.strong("");
                    ui.strong(t("gui.col_skill"));
                    ui.strong(t("gui.col_installed"));
                    ui.strong(t("gui.col_latest"));
                    ui.strong(t("gui.col_state"));
                    ui.end_row();
                    for r in self.rows.clone() {
                        // checkbox de seleção
                        let mut on = self.selected.contains(&r.name);
                        if ui.add_enabled(!busy, egui::Checkbox::new(&mut on, "")).changed() {
                            if on {
                                self.selected.insert(r.name.clone());
                            } else {
                                self.selected.remove(&r.name);
                            }
                        }
                        ui.label(&r.name);
                        ui.label(r.installed.clone().unwrap_or_else(|| "—".into()));
                        ui.label(r.latest.clone().unwrap_or_else(|| "?".into()));
                        // estado (cor conforme)
                        let txt = state_text(&r);
                        let col = if r.missing() {
                            egui::Color32::from_rgb(0xe6, 0xb2, 0x3a)
                        } else if r.outdated() {
                            egui::Color32::from_rgb(0x5b, 0x8c, 0xff)
                        } else {
                            egui::Color32::from_rgb(0x8a, 0x90, 0xa2)
                        };
                        ui.label(egui::RichText::new(txt).color(col));
                        ui.end_row();
                    }
                });
            });
        });

        egui::Panel::bottom("bottom").resizable(false).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(t("gui.agent_on")).clicked() {
                    self.status = shell("schematize autostart enable");
                }
                if ui.button(t("gui.agent_off")).clicked() {
                    self.status = shell("schematize autostart disable");
                }
                if ui.button(t("gui.overdev_on")).clicked() {
                    self.status = shell("schematize overdev enable");
                }
                if ui.button(t("gui.panel")).clicked() {
                    self.status = shell("schematize panel");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("gui.github")).clicked() {
                        util::open_url(links::GITHUB);
                    }
                    if ui.button(t("gui.blog")).clicked() {
                        util::open_url(links::BLOG);
                    }
                    if ui.button(t("gui.site")).clicked() {
                        util::open_url(links::SITE);
                    }
                });
            });
            ui.add_space(6.0);
        });
    }
}

fn main() -> eframe::Result<()> {
    let (rgba, w, h) = schematize::appicon::rgba(256);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([820.0, 560.0])
            .with_min_inner_size([620.0, 400.0])
            .with_title("schematize")
            .with_icon(IconData { rgba, width: w, height: h }),
        ..Default::default()
    };
    eframe::run_native("schematize", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
