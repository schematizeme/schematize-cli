//! schematize-gui — janela de gerenciamento (egui/eframe 0.36). O quê: lista skills
//! (instalada vs latest) com botões de atualizar, troca de idioma, liga o agente
//! (autostart) e o overdev, e abre site/blog/GitHub. Onde: binário separado
//! (feature `gui`); usa a lib `schematize`. Precisa de libs X11/Wayland/GL (ver README).

use eframe::egui;
use schematize::i18n::{self, t, tf};
use schematize::{links, registry, skills, util};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

const CLI_INSTALL: &str =
    "curl -fsSL https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh | bash";

/// Uma linha da tabela: uma skill (com slug) ou o próprio CLI (slug None).
#[derive(Clone)]
struct Row {
    name: String,
    installed: Option<String>,
    latest: Option<String>,
    slug: Option<String>, // None = CLI
}

impl Row {
    fn outdated(&self) -> bool {
        matches!((&self.installed, &self.latest), (Some(i), Some(l)) if i != l)
    }
    fn missing(&self) -> bool {
        self.installed.is_none()
    }
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
            slug: Some(it.slug),
        })
        .collect();
    rows.push(Row {
        name: "schematize (CLI)".into(),
        installed: Some(env!("CARGO_PKG_VERSION").to_string()),
        latest: skills::latest_release_tag("schematize-cli"),
        slug: None,
    });
    rows
}

/// Texto de status a partir das linhas (localizado).
fn status_for(rows: &[Row]) -> String {
    let n = rows.iter().filter(|r| r.outdated() || r.missing()).count();
    if n == 0 {
        t("gui.all_uptodate")
    } else {
        tf("gui.n_pending", &[("n", &n.to_string())])
    }
}

struct App {
    rows: Vec<Row>,
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

    /// Instala/atualiza uma linha (skill in-process; CLI via bootstrap) numa thread.
    fn update_row(&self, row: Row, ctx: egui::Context) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            match row.slug.as_deref().and_then(|sl| registry::find(&registry::catalog(), sl)) {
                Some(it) => {
                    let _ = skills::install(&it);
                }
                None => {
                    let _ = util::run("bash", &["-c", CLI_INSTALL]);
                }
            }
            let rows = collect_rows();
            let st = status_for(&rows);
            let _ = tx.send((rows, st));
            ctx.request_repaint();
        });
    }

    fn update_all(&self, ctx: egui::Context) {
        for r in self.rows.clone().into_iter().filter(|r| r.outdated() || r.missing()) {
            self.update_row(r, ctx.clone());
        }
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
                    if ui.add_enabled(!busy, egui::Button::new(t("gui.update_all"))).clicked() {
                        self.update_all(ctx.clone());
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(if busy { t("gui.working") } else { self.status.clone() });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("skills").num_columns(4).striped(true).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.strong(t("gui.col_skill"));
                    ui.strong(t("gui.col_installed"));
                    ui.strong(t("gui.col_latest"));
                    ui.strong("");
                    ui.end_row();
                    for r in self.rows.clone() {
                        ui.label(&r.name);
                        ui.label(r.installed.clone().unwrap_or_else(|| "—".into()));
                        ui.label(r.latest.clone().unwrap_or_else(|| "?".into()));
                        if r.missing() {
                            if ui.add_enabled(!busy, egui::Button::new(t("gui.install"))).clicked() {
                                self.update_row(r.clone(), ctx.clone());
                            }
                        } else if r.outdated() {
                            if ui.add_enabled(!busy, egui::Button::new(t("gui.update"))).clicked() {
                                self.update_row(r.clone(), ctx.clone());
                            }
                        } else {
                            ui.label(egui::RichText::new(t("gui.current")).weak());
                        }
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([780.0, 540.0])
            .with_min_inner_size([560.0, 380.0])
            .with_title("schematize"),
        ..Default::default()
    };
    eframe::run_native("schematize", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
