//! schematize-gui — a JANELA onde se faz a schematização: um hub com abas.
//! O quê: (1) Skills — gestor (checkbox + instalar/atualizar/remover em massa/paralelo);
//! (2) Overdev — o run do projeto (objetivo, checklist, decisões, plano, perguntas), NATIVO;
//! (3) Grafo — a tela de grafos do index (force-directed estilo Obsidian), NATIVA, nós
//! linkados a arquivo:linha. Tudo dentro do app, persistente (nada de aba de navegador que
//! some). Onde: módulo do lib (feature `gui`), aberto por `schematize gui` ou pelo atalho `schematize-gui`.

use eframe::egui;
use eframe::egui::IconData;
use notify_rust::Notification;
use crate::i18n::{self, t, tf};
use crate::registry::Item;
use crate::{config, links, panel, projects, registry, selfupdate, skills, util};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

// ----------------------------------------------------------------------------
// Aba do hub.
// ----------------------------------------------------------------------------
#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Skills,
    Overdev,
    Graph,
}

// ----------------------------------------------------------------------------
// Skills (gestor) — igual à versão anterior.
// ----------------------------------------------------------------------------
#[derive(Clone)]
struct Row {
    name: String,
    installed: Option<String>,
    latest: Option<String>,
    item: Option<Item>,
    category: String,                  // "base" | "language" | "external"
    sponsor: Option<(String, String)>, // (nome, url) do autor
    verified: bool,                    // selo de verificado
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
enum Op {
    Install(Item),
    Remove(Item),
    SelfUpdate,
}
/// Resultado de um lote entregue pela thread coordenadora à UI.
struct Msg {
    rows: Vec<Row>,
    status: String,
    /// Some(..) se um self-update do CLI/GUI rodou no lote (Ok=msg, Err=motivo).
    selfupdate: Option<Result<String, String>>,
}
fn collect_rows() -> Vec<Row> {
    let mut rows: Vec<Row> = registry::catalog()
        .into_iter()
        .map(|it| {
            let sponsor = it.sponsor.as_ref().map(|s| (s.name.clone(), s.url.clone()));
            let category = if it.category.is_empty() { "language".into() } else { it.category.clone() };
            let verified = it.verified;
            Row {
                name: it.slug.clone(),
                installed: skills::installed_version(&it),
                latest: skills::resolve_latest(&it).ok(),
                category,
                sponsor,
                verified,
                item: Some(it),
            }
        })
        .collect();
    rows.push(Row {
        name: "schematize (CLI)".into(),
        installed: Some(env!("CARGO_PKG_VERSION").to_string()),
        latest: skills::latest_version_raw("schematize-cli"),
        item: None,
        category: "base".into(),
        sponsor: Some(("Lucassa".into(), "https://lucassa.me".into())),
        verified: true,
    });
    rows
}
fn status_for(rows: &[Row]) -> String {
    let n = rows.iter().filter(|r| r.pending()).count();
    if n == 0 {
        t("gui.all_uptodate")
    } else {
        tf("gui.n_pending", &[("n", &n.to_string())])
    }
}
fn state_text(r: &Row) -> String {
    if r.missing() {
        t("common.not_installed")
    } else if r.outdated() {
        t("common.update")
    } else {
        t("common.current")
    }
}

// ----------------------------------------------------------------------------
// Grafo — nó com estado de simulação.
// ----------------------------------------------------------------------------
struct GNode {
    id: String,
    loc: Option<String>,
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    deg: f32,
}

/// Carrega fontes de sistema que cobrem CJK/árabe/devanagari/etc como FALLBACK — mata os
/// "quadradinhos" (tofu) nos idiomas não-latinos. Latino usa a fonte padrão; o resto cai aqui.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates: &[&str] = &[
        // Linux — Noto (fonts-noto / fonts-noto-cjk)
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
        // Tailandês (th) e Bengali (bn) — fonts-noto-core no Debian. Coreano (Hangul)
        // vem do CJK acima; Persa (fa) usa a escrita árabe (já coberta acima).
        "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansThai-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansBengali-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansBengali-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        // Linux — DejaVu (quase sempre presente): latino/cirílico/grego
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\arialuni.ttf",
    ];
    let mut added = Vec::new();
    for (i, path) in candidates.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(path) {
            let name = format!("sys-{i}");
            fonts.font_data.insert(name.clone(), egui::FontData::from_owned(bytes).into());
            added.push(name);
        }
    }
    if !added.is_empty() {
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let list = fonts.families.entry(fam).or_default();
            for n in &added {
                list.push(n.clone());
            }
        }
    }
    ctx.set_fonts(fonts);
}

/// Um tema um pouco menos "cru": fontes maiores, mais respiro e um azul de acento.
fn install_style(ctx: &egui::Context) {
    use egui::FontFamily::{Monospace, Proportional};
    use egui::{FontId, TextStyle};
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Heading, FontId::new(21.0, Proportional)),
            (TextStyle::Body, FontId::new(14.5, Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, Monospace)),
            (TextStyle::Button, FontId::new(14.5, Proportional)),
            (TextStyle::Small, FontId::new(11.5, Proportional)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(10.0, 9.0);
        s.spacing.button_padding = egui::vec2(11.0, 6.0);
        let accent = egui::Color32::from_rgb(0x5b, 0x8c, 0xff);
        s.visuals.hyperlink_color = accent;
        s.visuals.selection.bg_fill = egui::Color32::from_rgb(0x2a, 0x3a, 0x66);
        s.visuals.selection.stroke = egui::Stroke::new(1.0, accent);
    });
}

struct App {
    // skills
    rows: Vec<Row>,
    selected: HashSet<String>,
    status: String,
    lang: String,
    busy: Arc<AtomicBool>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    // feedback de self-update
    flash: String,
    flash_err: bool,
    restart: bool,
    // hub
    tab: Tab,
    project: Option<PathBuf>,
    recent: Vec<String>,
    // diretórios de desenvolvimento + projetos descobertos por marcadores
    dev_dirs: Vec<String>,
    projects: Vec<projects::Project>,
    // overdev
    overdev: Option<panel::Overdev>,
    // grafo
    gnodes: Vec<GNode>,
    gedges: Vec<(usize, usize)>,
    graph_index: Option<PathBuf>,
    gsel: Option<usize>,
    gsearch: String,
    scale: f32,
    ox: f32,
    oy: f32,
    alpha: f32,
    drag_node: Option<usize>,
    fit_pending: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        install_fonts(&cc.egui_ctx); // mata os "quadradinhos" dos idiomas não-latinos
        install_style(&cc.egui_ctx); // menos cru: fontes/respiro/acento
        let (tx, rx) = channel();
        let mut app = Self {
            rows: vec![],
            selected: HashSet::new(),
            status: t("gui.loading"),
            lang: i18n::current_code(),
            busy: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            flash: String::new(),
            flash_err: false,
            restart: false,
            tab: Tab::Skills,
            project: None,
            recent: config::recent_projects(),
            dev_dirs: config::dev_dirs(),
            projects: vec![],
            overdev: None,
            gnodes: vec![],
            gedges: vec![],
            graph_index: None,
            gsel: None,
            gsearch: String::new(),
            scale: 1.0,
            ox: 0.0,
            oy: 0.0,
            alpha: 1.0,
            drag_node: None,
            fit_pending: false,
        };
        app.refresh(cc.egui_ctx.clone());
        app.projects = projects::scan(&app.dev_dirs); // descobre projetos dos dev_dirs
        // projeto inicial: o mais recente, ou o cwd se tiver .overdev/_archive.
        if let Some(p) = app.recent.first().cloned() {
            app.set_project(PathBuf::from(p));
        } else if let Ok(cwd) = std::env::current_dir() {
            if cwd.join(".overdev").is_dir() || panel::find_index_dir(&cwd).is_some() {
                app.set_project(cwd);
            }
        }
        app
    }

    fn refresh(&self, ctx: egui::Context) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let rows = collect_rows();
            let st = status_for(&rows);
            let _ = tx.send(Msg { rows, status: st, selfupdate: None });
            ctx.request_repaint();
        });
    }

    fn run_batch(&self, ops: Vec<Op>, ctx: egui::Context) {
        if ops.is_empty() || self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // skills em paralelo; self-update à parte pra capturar o resultado.
            std::thread::scope(|sc| {
                for op in &ops {
                    match op {
                        Op::Install(it) => {
                            sc.spawn(move || {
                                let _ = skills::install(it);
                            });
                        }
                        Op::Remove(it) => {
                            sc.spawn(move || {
                                let _ = skills::remove(it);
                            });
                        }
                        Op::SelfUpdate => {}
                    }
                }
            });
            let selfupdate = if ops.iter().any(|o| matches!(o, Op::SelfUpdate)) {
                // binário+pkexec (gráfico) → fallback terminal do fonte. Nunca no-op.
                let res = update_cli();
                let r2 = res.clone();
                // notificação de desktop numa thread própria (wait_for_action bloqueia).
                std::thread::spawn(move || notify_selfupdate(&r2));
                Some(res)
            } else {
                None
            };
            let rows = collect_rows();
            let st = status_for(&rows);
            let _ = tx.send(Msg { rows, status: st, selfupdate });
            ctx.request_repaint();
        });
    }
    fn apply_op(r: &Row) -> Op {
        match &r.item {
            Some(it) => Op::Install(it.clone()),
            None => Op::SelfUpdate,
        }
    }
    fn ops_all_pending(&self) -> Vec<Op> {
        self.rows.iter().filter(|r| r.pending()).map(Self::apply_op).collect()
    }
    fn ops_apply_selected(&self) -> Vec<Op> {
        self.rows
            .iter()
            .filter(|r| self.selected.contains(&r.name) && r.pending())
            .map(Self::apply_op)
            .collect()
    }
    fn ops_remove_selected(&self) -> Vec<Op> {
        self.rows
            .iter()
            .filter(|r| self.selected.contains(&r.name) && !r.missing())
            .filter_map(|r| r.item.clone().map(Op::Remove))
            .collect()
    }
    fn count_apply(&self) -> usize {
        self.rows.iter().filter(|r| self.selected.contains(&r.name) && r.pending()).count()
    }
    fn count_remove(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| self.selected.contains(&r.name) && !r.missing() && r.item.is_some())
            .count()
    }

    // ---- diretórios de desenvolvimento + descoberta ----
    /// Recarrega a lista de dev_dirs da config e re-varre os projetos por marcadores.
    fn rescan_projects(&mut self) {
        self.dev_dirs = config::dev_dirs();
        self.projects = projects::scan(&self.dev_dirs);
    }

    // ---- projeto ----
    fn set_project(&mut self, path: PathBuf) {
        let abs = std::fs::canonicalize(&path).unwrap_or(path);
        config::add_recent_project(&abs.to_string_lossy());
        self.recent = config::recent_projects();
        self.project = Some(abs);
        self.reload_project();
    }
    fn reload_project(&mut self) {
        let Some(proj) = self.project.clone() else { return };
        self.overdev = Some(panel::load_overdev(&proj));
        let (nodes, edges, idx) = panel::load_graph(&proj);
        let mut id_to_idx: HashMap<String, usize> = HashMap::new();
        self.gnodes = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                id_to_idx.insert(n.id.clone(), i);
                let a = i as f32 * 2.399_963;
                let r = 40.0 + 9.0 * (i as f32).sqrt();
                GNode { id: n.id.clone(), loc: n.loc.clone(), x: a.cos() * r, y: a.sin() * r, vx: 0.0, vy: 0.0, deg: 0.0 }
            })
            .collect();
        self.gedges = edges
            .iter()
            .filter_map(|e| Some((*id_to_idx.get(&e.from)?, *id_to_idx.get(&e.to)?)))
            .collect();
        for &(a, b) in &self.gedges {
            self.gnodes[a].deg += 1.0;
            self.gnodes[b].deg += 1.0;
        }
        self.graph_index = idx;
        self.gsel = None;
        self.alpha = 1.0;
        self.scale = 1.0;
        self.ox = 0.0;
        self.oy = 0.0;
        self.fit_pending = true;
    }

    // ---- física do grafo ----
    fn step_graph(&mut self) {
        if self.alpha < 0.02 {
            return;
        }
        let n = self.gnodes.len();
        const REP: f32 = 1400.0;
        const SPR: f32 = 0.02;
        const LEN: f32 = 70.0;
        const G: f32 = 0.015;
        for i in 0..n {
            let (xi, yi) = (self.gnodes[i].x, self.gnodes[i].y);
            let (mut fx, mut fy) = (0.0f32, 0.0f32);
            for j in 0..n {
                if i == j {
                    continue;
                }
                let dx = xi - self.gnodes[j].x;
                let dy = yi - self.gnodes[j].y;
                let d2 = dx * dx + dy * dy + 0.01;
                let d = d2.sqrt();
                let f = REP / d2;
                fx += f * dx / d;
                fy += f * dy / d;
            }
            fx -= xi * G;
            fy -= yi * G;
            self.gnodes[i].vx += fx;
            self.gnodes[i].vy += fy;
        }
        for &(a, b) in &self.gedges {
            let dx = self.gnodes[b].x - self.gnodes[a].x;
            let dy = self.gnodes[b].y - self.gnodes[a].y;
            let d = (dx * dx + dy * dy).sqrt().max(0.01);
            let f = (d - LEN) * SPR;
            let (fx, fy) = (f * dx / d, f * dy / d);
            self.gnodes[a].vx += fx;
            self.gnodes[a].vy += fy;
            self.gnodes[b].vx -= fx;
            self.gnodes[b].vy -= fy;
        }
        for nd in &mut self.gnodes {
            nd.vx *= 0.86;
            nd.vy *= 0.86;
            nd.x += nd.vx * self.alpha;
            nd.y += nd.vy * self.alpha;
        }
        self.alpha *= 0.994;
    }
}

fn shell(cmd: &str) -> String {
    match util::run("bash", &["-lc", cmd]) {
        Ok(o) => o.trim().to_string(),
        Err(e) => format!("erro: {e}"),
    }
}

fn which(cmd: &str) -> bool {
    util::run("sh", &["-c", &format!("command -v {cmd}")]).is_ok()
}

/// Abre um terminal gráfico rodando o upgrade do fonte — fallback quando não há binário do
/// release (CI falho) ou o swap falha. Assim o leigo nunca fica com botão que "não faz nada".
fn launch_terminal_upgrade() -> bool {
    let inner = "echo '── Atualizando o schematize ──'; echo 'Pode pedir sua senha; leva alguns minutos.'; echo; \
                 curl -fsSL https://raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh | bash -s -- --from-source; \
                 echo; echo 'Pronto! Feche esta janela e reabra o schematize.'; \
                 read -n1 -s -r -p 'Pressione uma tecla para fechar...'";
    // (emulador, prefixo antes de `bash -c`) — cobre os terminais comuns de KDE/GNOME/XFCE/etc.
    let cands: &[(&str, &[&str])] = &[
        ("x-terminal-emulator", &["-e"]),
        ("konsole", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("xfce4-terminal", &["-x"]),
        ("tilix", &["-e"]),
        ("kitty", &[]),
        ("alacritty", &["-e"]),
        ("xterm", &["-e"]),
    ];
    for (term, pre) in cands {
        if which(term) && std::process::Command::new(term).args(*pre).arg("bash").arg("-c").arg(inner).spawn().is_ok() {
            return true;
        }
    }
    false
}

/// Atualiza o CLI/GUI: binário + pkexec (gráfico, sem terminal) primeiro; se falhar (sem
/// binário no release / erro), abre um TERMINAL fazendo o rebuild do fonte. Nunca no-op.
fn update_cli() -> Result<String, String> {
    match selfupdate::run() {
        Ok(m) => Ok(m),
        Err(e) => {
            if launch_terminal_upgrade() {
                Ok("Abri um terminal pra atualizar — siga ali (pode pedir a senha). Depois reabra a janela.".into())
            } else {
                Err(format!("{e} — e não achei um terminal pra abrir. No terminal, rode: schematize upgrade"))
            }
        }
    }
}

/// Relança a GUI (o binário no lugar já é o novo, pós-self-update) e encerra esta.
fn restart_gui() -> ! {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    // DESACOPLA o filho (novo processo/grupo + stdio nulo): senão ele morre junto quando
    // o processo atual sai — o bug "reinicia só fecha, não reabre a janela".
    let exe = std::env::current_exe().unwrap_or_else(|_| "schematize-gui".into());
    let _ = std::process::Command::new(exe)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    std::process::exit(0);
}

/// Notificação de desktop do resultado do self-update, com botão "Reiniciar" no sucesso.
fn notify_selfupdate(res: &Result<String, String>) {
    let mut n = Notification::new();
    n.icon("schematize");
    let has_restart = match res {
        Ok(m) => {
            n.summary(&t("gui.su_ok_title")).body(&format!("{m}\n{}", t("gui.su_restart_hint")));
            let up = m.contains("Atualizado para");
            if up {
                n.action("restart", &t("gui.restart_btn"));
            }
            up
        }
        Err(e) => {
            n.summary(&t("gui.su_err_title")).body(e);
            false
        }
    };
    if let Ok(h) = n.timeout(0).show() {
        if has_restart {
            h.wait_for_action(|a| {
                if a == "restart" {
                    restart_gui();
                }
            });
        }
    }
}
fn nsize(deg: f32) -> f32 {
    3.0 + (deg.sqrt() * 1.7).min(9.0)
}

/// Nome curto (basename) de um caminho pra exibir no seletor; cai no caminho inteiro.
fn basename_of(p: &PathBuf) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            self.rows = msg.rows;
            self.status = msg.status;
            self.busy.store(false, Ordering::SeqCst);
            let names: HashSet<String> = self.rows.iter().map(|r| r.name.clone()).collect();
            self.selected.retain(|n| names.contains(n));
            if let Some(res) = msg.selfupdate {
                match res {
                    Ok(m) => {
                        self.restart = m.contains("Atualizado para");
                        self.flash = m;
                        self.flash_err = false;
                    }
                    Err(e) => {
                        self.flash = e;
                        self.flash_err = true;
                        self.restart = false;
                    }
                }
            }
        }
        let ctx = ui.ctx().clone();

        // ---- topo: título + idioma + abas ----
        egui::Panel::top("top").resizable(false).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("schematize");
                ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());
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
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.selectable_label(self.tab == Tab::Skills, t("gui.tab_skills")).clicked() {
                    self.tab = Tab::Skills;
                }
                if ui.selectable_label(self.tab == Tab::Overdev, t("gui.tab_overdev")).clicked() {
                    self.tab = Tab::Overdev;
                }
                if ui.selectable_label(self.tab == Tab::Graph, t("gui.tab_graph")).clicked() {
                    self.tab = Tab::Graph;
                }
            });
            // banner de feedback do self-update (resultado + pedido de reiniciar)
            if self.restart || !self.flash.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let fg = if self.flash_err {
                        egui::Color32::from_rgb(0xff, 0x8a, 0x8a)
                    } else {
                        egui::Color32::from_rgb(0x3e, 0xcf, 0x8e)
                    };
                    let txt = if self.restart { format!("✓ {}  ·  {}", self.flash, t("gui.su_restart_hint")) } else { self.flash.clone() };
                    ui.colored_label(fg, txt);
                    if self.restart && ui.button(t("gui.restart_btn")).clicked() {
                        restart_gui();
                    }
                    if ui.small_button("✕").clicked() {
                        self.flash.clear();
                        self.restart = false;
                    }
                });
            }
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Skills => self.skills_view(ui, &ctx),
            Tab::Overdev => {
                self.project_bar(ui);
                self.overdev_view(ui);
            }
            Tab::Graph => {
                self.project_bar(ui);
                self.graph_view(ui, &ctx);
            }
        });

        egui::Panel::bottom("bottom").resizable(false).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(t("gui.agent_on")).clicked() {
                    self.status = shell("schematize autostart enable");
                }
                if ui.button(t("gui.overdev_on")).clicked() {
                    self.status = shell("schematize overdev enable");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("gui.github")).clicked() {
                        util::open_url(links::GITHUB);
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

impl App {
    // ---- barra de seleção de projeto (abas Overdev/Grafo) ----
    fn project_bar(&mut self, ui: &mut egui::Ui) {
        // Linha 1: seletor de projeto (descobertos por marcadores + recentes) + ações.
        ui.horizontal(|ui| {
            ui.label(t("gui.project"));
            let cur = self
                .project
                .as_ref()
                .map(|p| basename_of(p))
                .unwrap_or_else(|| t("gui.no_project"));
            let mut pick: Option<String> = None;
            egui::ComboBox::from_id_salt("proj").width(360.0).selected_text(cur).show_ui(ui, |ui| {
                // Projetos descobertos nos diretórios de desenvolvimento.
                if !self.projects.is_empty() {
                    ui.label(egui::RichText::new(t("gui.detected_projects")).weak().small());
                    for pr in &self.projects {
                        let label = format!("{}   ·  {}", pr.name, pr.marker);
                        if ui.selectable_label(false, label).on_hover_text(&pr.path).clicked() {
                            pick = Some(pr.path.clone());
                        }
                    }
                }
                // Recentes que não estão já na lista de descobertos.
                let known: HashSet<&str> = self.projects.iter().map(|p| p.path.as_str()).collect();
                let recents: Vec<&String> = self.recent.iter().filter(|r| !known.contains(r.as_str())).collect();
                if !recents.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new(t("gui.recent_projects")).weak().small());
                    for p in recents {
                        if ui.selectable_label(false, basename_of(&PathBuf::from(p))).on_hover_text(p).clicked() {
                            pick = Some(p.clone());
                        }
                    }
                }
            });
            if let Some(p) = pick {
                self.set_project(PathBuf::from(p));
            }
            // Abrir uma pasta avulsa por vez (seletor NATIVO do sistema).
            if ui.button(t("gui.open_folder")).clicked() {
                if let Some(dir) = rfd::FileDialog::new().set_title(t("gui.open_folder")).pick_folder() {
                    self.set_project(dir);
                }
            }
            if ui.button(t("gui.reload")).clicked() {
                self.rescan_projects();
                self.reload_project();
            }
        });

        // Linha 2: gestão dos diretórios de desenvolvimento.
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(t("gui.dev_dirs")).strong());
            if ui.button(t("gui.add_dev_dir")).clicked() {
                if let Some(dir) = rfd::FileDialog::new().set_title(t("gui.add_dev_dir")).pick_folder() {
                    let abs = std::fs::canonicalize(&dir).unwrap_or(dir);
                    config::add_dev_dir(&abs.to_string_lossy());
                    self.rescan_projects();
                }
            }
        });
        if self.dev_dirs.is_empty() {
            ui.label(egui::RichText::new(t("gui.dev_dirs_empty")).weak());
        } else {
            let mut remove: Option<String> = None;
            for d in &self.dev_dirs {
                ui.horizontal(|ui| {
                    if ui.small_button("✕").on_hover_text(t("gui.remove")).clicked() {
                        remove = Some(d.clone());
                    }
                    ui.label(egui::RichText::new(d).monospace().weak());
                });
            }
            if let Some(d) = remove {
                config::remove_dev_dir(&d);
                self.rescan_projects();
            }
        }
        ui.separator();
    }

    // ---- aba Skills (gestor) ----
    fn skills_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy.load(Ordering::SeqCst);
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new(t("gui.check"))).clicked() {
                self.refresh(ctx.clone());
            }
            let has_pending = self.rows.iter().any(|r| r.pending());
            if ui.add_enabled(!busy && has_pending, egui::Button::new(t("gui.update_all"))).clicked() {
                self.run_batch(self.ops_all_pending(), ctx.clone());
            }
            ui.separator();
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
        });
        ui.horizontal(|ui| {
            let na = self.count_apply();
            let nr = self.count_remove();
            if ui.add_enabled(!busy && na > 0, egui::Button::new(format!("{} ({na})", t("gui.install_sel")))).clicked() {
                let ops = self.ops_apply_selected();
                self.run_batch(ops, ctx.clone());
            }
            if ui.add_enabled(!busy && nr > 0, egui::Button::new(format!("{} ({nr})", t("gui.remove_sel")))).clicked() {
                let ops = self.ops_remove_selected();
                self.run_batch(ops, ctx.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(if busy { t("gui.working") } else { self.status.clone() });
            });
        });
        ui.separator();
        let rows = self.rows.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (cat, label) in [
                ("base", t("gui.cat_base")),
                ("language", t("gui.cat_language")),
                ("external", t("gui.cat_external")),
            ] {
                let group: Vec<Row> = rows.iter().filter(|r| r.category == cat).cloned().collect();
                if group.is_empty() {
                    continue;
                }
                ui.add_space(10.0);
                ui.label(egui::RichText::new(label).strong().size(14.0).color(egui::Color32::from_rgb(0x9d, 0xb4, 0xff)));
                ui.add_space(2.0);
                egui::Grid::new(format!("skills_{cat}")).num_columns(6).striped(true).spacing([18.0, 10.0]).show(ui, |ui| {
                    ui.label("");
                    ui.strong(t("gui.col_skill"));
                    ui.strong(t("gui.col_author"));
                    ui.strong(t("gui.col_installed"));
                    ui.strong(t("gui.col_latest"));
                    ui.strong(t("gui.col_state"));
                    ui.end_row();
                    for r in &group {
                        let mut on = self.selected.contains(&r.name);
                        if ui.add_enabled(!busy, egui::Checkbox::new(&mut on, "")).changed() {
                            if on {
                                self.selected.insert(r.name.clone());
                            } else {
                                self.selected.remove(&r.name);
                            }
                        }
                        // skill + selo verificado
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&r.name).strong());
                            if r.verified {
                                ui.label(egui::RichText::new("✓").color(egui::Color32::from_rgb(0x3e, 0xcf, 0x8e)))
                                    .on_hover_text(t("gui.verified"));
                            }
                        });
                        // autor (coluna própria, link clicável)
                        match &r.sponsor {
                            Some((sn, su)) => {
                                ui.hyperlink_to(egui::RichText::new(sn).weak(), su);
                            }
                            None => {
                                ui.label("—");
                            }
                        }
                        ui.label(r.installed.clone().unwrap_or_else(|| "—".into()));
                        ui.label(r.latest.clone().unwrap_or_else(|| "?".into()));
                        let col = if r.missing() {
                            egui::Color32::from_rgb(0xe6, 0xb2, 0x3a)
                        } else if r.outdated() {
                            egui::Color32::from_rgb(0x5b, 0x8c, 0xff)
                        } else {
                            egui::Color32::from_rgb(0x8a, 0x90, 0xa2)
                        };
                        ui.label(egui::RichText::new(state_text(r)).color(col));
                        ui.end_row();
                    }
                });
            }
        });
    }

    // ---- aba Overdev (nativa) ----
    fn overdev_view(&mut self, ui: &mut egui::Ui) {
        let Some(ov) = &self.overdev else {
            ui.label(t("gui.no_project"));
            return;
        };
        if ov.objetivo.trim().is_empty() && ov.items.is_empty() {
            ui.label(t("gui.no_overdev"));
            return;
        }
        let proj = self.project.clone();
        ui.horizontal(|ui| {
            ui.heading(if ov.objetivo.trim().is_empty() { t("gui.no_overdev") } else { ov.objetivo.clone() });
            let m = egui::RichText::new(&ov.mode);
            ui.label(if ov.mode == "active" { m.color(egui::Color32::from_rgb(0x3e, 0xcf, 0x8e)) } else { m.weak() });
        });
        let (o, d, h) = ov.counts();
        ui.label(format!("✓ {d}   ○ {o}   ~ {h}"));
        if ui.button(t("gui.open_browser")).on_hover_text("HTML").clicked() {
            if let Some(p) = &proj {
                let _ = panel::open_in_browser(p);
            }
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (s, txt) in &ov.items {
                let (mark, col) = match s {
                    'x' => ("✓", egui::Color32::from_rgb(0x3e, 0xcf, 0x8e)),
                    '~' => ("~", egui::Color32::from_rgb(0xe6, 0xb2, 0x3a)),
                    _ => ("○", egui::Color32::from_rgb(0x8a, 0x90, 0xa2)),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(mark).color(col).monospace());
                    ui.label(txt);
                });
            }
            let sec = |ui: &mut egui::Ui, title: &str, body: &str| {
                if !body.trim().is_empty() {
                    egui::CollapsingHeader::new(title).show(ui, |ui| {
                        ui.label(egui::RichText::new(body).monospace().weak());
                    });
                }
            };
            ui.add_space(6.0);
            sec(ui, &t("gui.od_decisions"), &ov.decisoes);
            sec(ui, &t("gui.od_plan"), &ov.plano);
            sec(ui, &t("gui.od_questions"), &ov.perguntas);
        });
    }

    // ---- aba Grafo (nativa, force-directed) ----
    fn graph_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.project.is_none() {
            ui.label(t("gui.no_project"));
            return;
        }
        if self.gnodes.is_empty() {
            ui.label(t("gui.no_graph"));
            return;
        }
        let proj = self.project.clone().unwrap();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.gsearch).hint_text(t("gui.search")).desired_width(200.0));
            ui.label(format!("{} {}", self.gnodes.len(), t("gui.graph_nodes")));
            if ui.button(t("gui.fit")).clicked() {
                self.fit_pending = true;
            }
            if ui.button(t("gui.export_obsidian")).clicked() {
                match panel::export_obsidian_at(&proj, None) {
                    Ok(dir) => {
                        self.status = tf("gui.exported", &[("p", &dir.to_string_lossy())]);
                        util::open_url(&dir.to_string_lossy());
                    }
                    Err(e) => self.status = format!("erro: {e}"),
                }
            }
            if ui.button(t("gui.open_browser")).clicked() {
                let _ = panel::open_in_browser(&proj);
            }
        });

        self.step_graph();
        if self.alpha >= 0.02 {
            ctx.request_repaint();
        }

        let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;
        let center = rect.center();
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(0x14, 0x16, 0x1c));
        if self.fit_pending {
            self.fit(rect);
            self.fit_pending = false;
        }
        // zoom (roda) em torno do ponteiro
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            if let Some(pos) = resp.hover_pos() {
                let m = pos - center - egui::vec2(self.ox, self.oy);
                let f = (scroll * 0.0015).exp();
                self.ox -= m.x * (f - 1.0);
                self.oy -= m.y * (f - 1.0);
                self.scale = (self.scale * f).clamp(0.12, 6.0);
            }
        }
        // arrastar nó / fundo
        if resp.drag_started() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let w = egui::vec2((pos.x - center.x - self.ox) / self.scale, (pos.y - center.y - self.oy) / self.scale);
                self.drag_node = self.hit(w.x, w.y);
            }
        }
        if resp.dragged() {
            let dd = resp.drag_delta();
            match self.drag_node {
                Some(i) => {
                    self.gnodes[i].x += dd.x / self.scale;
                    self.gnodes[i].y += dd.y / self.scale;
                    self.gnodes[i].vx = 0.0;
                    self.gnodes[i].vy = 0.0;
                    self.alpha = self.alpha.max(0.3);
                }
                None => {
                    self.ox += dd.x;
                    self.oy += dd.y;
                }
            }
        }
        if resp.drag_stopped() {
            self.drag_node = None;
        }
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let w = egui::vec2((pos.x - center.x - self.ox) / self.scale, (pos.y - center.y - self.oy) / self.scale);
                self.gsel = self.hit(w.x, w.y);
            }
        }

        let q = self.gsearch.trim().to_lowercase();
        let focus = self.gsel;
        let nb: HashSet<usize> = match focus {
            Some(i) => self
                .gedges
                .iter()
                .filter_map(|&(a, b)| if a == i { Some(b) } else if b == i { Some(a) } else { None })
                .collect(),
            None => HashSet::new(),
        };
        // transform (locais — não borra self, liberando as mutações acima)
        let (scale, ox, oy) = (self.scale, self.ox, self.oy);
        let to_screen = |x: f32, y: f32| center + egui::vec2(x * scale + ox, y * scale + oy);
        // arestas
        for &(a, b) in &self.gedges {
            let on = focus == Some(a) || focus == Some(b);
            let col = if on {
                egui::Color32::from_rgba_unmultiplied(120, 150, 255, 200)
            } else {
                egui::Color32::from_rgba_unmultiplied(120, 130, 160, 40)
            };
            painter.line_segment(
                [to_screen(self.gnodes[a].x, self.gnodes[a].y), to_screen(self.gnodes[b].x, self.gnodes[b].y)],
                egui::Stroke::new(1.0, col),
            );
        }
        // nós
        for (i, n) in self.gnodes.iter().enumerate() {
            let matched = !q.is_empty() && n.id.to_lowercase().contains(&q);
            let dim = (focus.is_some() && focus != Some(i) && !nb.contains(&i)) || (!q.is_empty() && !matched);
            let r = nsize(n.deg);
            let base = if Some(i) == self.gsel {
                egui::Color32::WHITE
            } else if n.loc.is_some() {
                egui::Color32::from_rgb(0x5b, 0x8c, 0xff)
            } else {
                egui::Color32::from_rgb(0x8a, 0xa0, 0xff)
            };
            let col = if dim { base.gamma_multiply(0.28) } else { base };
            let p = to_screen(n.x, n.y);
            painter.circle_filled(p, r, col);
            if Some(i) == self.gsel {
                painter.circle_stroke(p, r + 1.5, egui::Stroke::new(2.0, egui::Color32::from_rgb(0x9d, 0xb4, 0xff)));
            }
            if self.scale > 1.15 || focus == Some(i) || nb.contains(&i) || matched {
                let label = if n.id.len() > 34 { format!("{}…", &n.id[..33]) } else { n.id.clone() };
                painter.text(
                    p + egui::vec2(r + 3.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::monospace(11.0),
                    if dim { egui::Color32::from_gray(120) } else { egui::Color32::from_rgb(0xc9, 0xce, 0xe0) },
                );
            }
        }
        // painel de info do nó selecionado (fundo pelo painter, widgets numa Area por cima)
        if let Some(i) = self.gsel {
            let (nid, nloc) = {
                let n = &self.gnodes[i];
                (n.id.clone(), n.loc.clone())
            };
            let info = egui::Rect::from_min_size(rect.left_bottom() + egui::vec2(8.0, -96.0), egui::vec2(420.0, 88.0));
            painter.rect_filled(info, 8.0, egui::Color32::from_rgb(0x1b, 0x1e, 0x27));
            let proj2 = proj.clone();
            egui::Area::new(egui::Id::new("node_info")).fixed_pos(info.min + egui::vec2(10.0, 8.0)).show(ctx, |ui| {
                ui.set_max_width(400.0);
                ui.strong(&nid);
                match &nloc {
                    Some(loc) => {
                        ui.label(egui::RichText::new(loc).monospace().color(egui::Color32::from_rgb(0x5b, 0x8c, 0xff)));
                        if ui.button(t("gui.open_editor")).clicked() {
                            let abs = std::fs::canonicalize(&proj2).unwrap_or_else(|_| proj2.clone());
                            util::open_url(&format!("vscode://file/{}/{}", abs.to_string_lossy(), loc));
                        }
                    }
                    None => {
                        ui.label(egui::RichText::new(t("gui.no_loc")).weak());
                    }
                }
            });
        }
    }

    fn hit(&self, wx: f32, wy: f32) -> Option<usize> {
        let mut best = None;
        let mut bd = f32::MAX;
        for (i, n) in self.gnodes.iter().enumerate() {
            let d = ((n.x - wx).powi(2) + (n.y - wy).powi(2)).sqrt();
            let r = nsize(n.deg) + 6.0 / self.scale;
            if d < r && d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    }

    fn fit(&mut self, rect: egui::Rect) {
        if self.gnodes.is_empty() {
            return;
        }
        let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for n in &self.gnodes {
            minx = minx.min(n.x);
            miny = miny.min(n.y);
            maxx = maxx.max(n.x);
            maxy = maxy.max(n.y);
        }
        let w = (maxx - minx).max(1.0);
        let h = (maxy - miny).max(1.0);
        self.scale = (0.86 * (rect.width() / w).min(rect.height() / h)).clamp(0.12, 6.0);
        self.ox = -self.scale * (minx + maxx) / 2.0;
        self.oy = -self.scale * (miny + maxy) / 2.0;
    }
}

pub fn run() -> eframe::Result<()> {
    let (rgba, w, h) = crate::appicon::rgba(256);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([680.0, 440.0])
            .with_title("schematize")
            .with_icon(IconData { rgba, width: w, height: h }),
        ..Default::default()
    };
    eframe::run_native("schematize", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
