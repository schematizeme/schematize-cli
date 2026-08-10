//! schematize-gui — janela de gerenciamento (egui/eframe). O quê: lista skills
//! (instalada vs latest) com botões de atualizar, liga o agente (autostart) e o
//! overdev. Onde: binário separado (feature `gui`); usa a lib `schematize`.
//! Precisa de libs X11/Wayland/GL no sistema (ver README).

use eframe::egui;
use schematize::{registry, skills, util};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const CLI_INSTALL: &str =
    "curl -fsSL https://github.com/schematizeme/schematize-cli/releases/latest/download/install.sh | bash";

/// Uma linha da tabela: uma skill (com slug) ou o próprio CLI (slug None).
#[derive(Clone)]
struct Row {
    name: String,
    installed: Option<String>,
    latest: Option<String>,
    slug: Option<&'static str>, // None = CLI
}

impl Row {
    fn outdated(&self) -> bool {
        matches!((&self.installed, &self.latest), (Some(i), Some(l)) if i != l)
    }
    fn missing(&self) -> bool {
        self.installed.is_none()
    }
}

struct App {
    rows: Vec<Row>,
    status: String,
    busy: Arc<AtomicBool>,
    tx: Sender<(Vec<Row>, String)>,
    rx: Receiver<(Vec<Row>, String)>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (tx, rx) = channel();
        let mut app = Self { rows: vec![], status: "carregando…".into(), busy: Arc::new(AtomicBool::new(false)), tx, rx };
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
            let mut rows: Vec<Row> = registry::ITEMS
                .iter()
                .map(|it| Row {
                    name: it.slug.to_string(),
                    installed: skills::installed_version(it),
                    latest: skills::resolve_latest(it).ok(),
                    slug: Some(it.slug),
                })
                .collect();
            rows.push(Row {
                name: "schematize (CLI)".into(),
                installed: Some(env!("CARGO_PKG_VERSION").to_string()),
                latest: skills::latest_release_tag("schematize-cli"),
                slug: None,
            });
            let n = rows.iter().filter(|r| r.outdated() || r.missing()).count();
            let st = if n == 0 { "tudo atualizado.".to_string() } else { format!("{n} com atualização/instalação pendente.") };
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
            match row.slug.and_then(registry::find) {
                Some(it) => {
                    let _ = skills::install(it);
                }
                None => {
                    let _ = util::run("bash", &["-c", CLI_INSTALL]);
                }
            }
            // recarrega o estado após atualizar.
            let mut rows: Vec<Row> = registry::ITEMS.iter().map(|it| Row {
                name: it.slug.to_string(),
                installed: skills::installed_version(it),
                latest: skills::resolve_latest(it).ok(),
                slug: Some(it.slug),
            }).collect();
            rows.push(Row { name: "schematize (CLI)".into(), installed: Some(env!("CARGO_PKG_VERSION").into()), latest: skills::latest_release_tag("schematize-cli"), slug: None });
            let _ = tx.send((rows, "atualizado.".into()));
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
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        while let Ok((rows, st)) = self.rx.try_recv() {
            self.rows = rows;
            self.status = st;
            self.busy.store(false, Ordering::SeqCst);
        }
        let busy = self.busy.load(Ordering::SeqCst);

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("schematize");
                ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Verificar")).clicked() {
                        self.refresh(ctx.clone());
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Atualizar tudo")).clicked() {
                        self.update_all(ctx.clone());
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(if busy { "trabalhando…".to_string() } else { self.status.clone() });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("skills").num_columns(4).striped(true).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.strong("skill");
                    ui.strong("instalada");
                    ui.strong("latest");
                    ui.strong("");
                    ui.end_row();
                    for r in self.rows.clone() {
                        ui.label(&r.name);
                        ui.label(r.installed.clone().unwrap_or_else(|| "—".into()));
                        ui.label(r.latest.clone().unwrap_or_else(|| "?".into()));
                        if r.missing() {
                            if ui.add_enabled(!busy, egui::Button::new("Instalar")).clicked() {
                                self.update_row(r.clone(), ctx.clone());
                            }
                        } else if r.outdated() {
                            if ui.add_enabled(!busy, egui::Button::new("Atualizar")).clicked() {
                                self.update_row(r.clone(), ctx.clone());
                            }
                        } else {
                            ui.label(egui::RichText::new("atual").weak());
                        }
                        ui.end_row();
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Ligar agente (autostart)").clicked() {
                    self.status = shell("schematize autostart enable");
                }
                if ui.button("Desligar agente").clicked() {
                    self.status = shell("schematize autostart disable");
                }
                if ui.button("Ligar overdev (hooks)").clicked() {
                    self.status = shell("schematize overdev enable");
                }
            });
            ui.add_space(6.0);
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 520.0])
            .with_min_inner_size([560.0, 360.0])
            .with_title("schematize"),
        ..Default::default()
    };
    eframe::run_native("schematize", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
