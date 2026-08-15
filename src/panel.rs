//! Painel auxiliar (HTML no browser) + export Obsidian do grafo do index.
//! O quê: `schematize panel` gera um HTML self-contained (sem CDN) com o estado do
//! overdev (objetivo, checklist, decisões, plano, perguntas parkeadas) e um grafo
//! force-directed estilo Obsidian do index (nós linkados a arquivo:linha); abre no
//! browser. `schematize graph obsidian` exporta o index como vault Obsidian
//! (markdown + [[wikilinks]]). Onde: chamado por main; lê `.overdev/*` e
//! `<projeto>_archive/index/*.md`. É read-mostly — o juiz do "terminou" segue sendo o
//! checklist+gate (overdev.rs); o painel só dá visão.

use crate::util::open_url;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Nó do grafo: id (função/serviço) e, se conhecido, `arquivo:linha`.
#[derive(Clone)]
pub struct Node {
    pub id: String,
    pub loc: Option<String>,
}
/// Aresta dirigida do grafo, com rótulo opcional (contrato/rota/evento).
#[derive(Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

/// Estado do overdev de um projeto (lido do control-plane) — consumido pela GUI e pelo HTML.
pub struct Overdev {
    pub objetivo: String,
    pub mode: String,
    pub items: Vec<(char, String)>, // (' '|'x'|'~', texto)
    pub decisoes: String,
    pub plano: String,
    pub perguntas: String,
}
impl Overdev {
    pub fn counts(&self) -> (u32, u32, u32) {
        let (mut o, mut d, mut h) = (0, 0, 0);
        for (s, _) in &self.items {
            match s {
                'x' => d += 1,
                '~' => h += 1,
                _ => o += 1,
            }
        }
        (o, d, h)
    }
}

// ---------------------------------------------------------------------------
// Descoberta do index e parsing do grafo (best-effort, tolerante a formato).
// ---------------------------------------------------------------------------

/// Procura `<algo>_archive/index/` sob `root` (e no pai de root). None se não achar.
pub fn find_index_dir(root: &Path) -> Option<PathBuf> {
    for base in [root.to_path_buf(), root.join("..")] {
        if let Ok(rd) = fs::read_dir(&base) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_dir() {
                    continue;
                }
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name.ends_with("_archive") {
                    let idx = p.join("index");
                    if idx.is_dir() {
                        return Some(idx);
                    }
                }
            }
        }
    }
    let direct = root.join("index");
    if direct.is_dir() {
        return Some(direct);
    }
    None
}

/// Limpa a decoração mermaid de um nó (`id[label]`, `id((label))`, aspas, crases).
fn clean_node(s: &str) -> String {
    let s = s.trim().trim_matches('`').trim();
    for (o, c) in [("[[", "]]"), ("((", "))"), ("[", "]"), ("(", ")"), ("{", "}")] {
        if let (Some(op), Some(cl)) = (s.find(o), s.rfind(c)) {
            if cl > op + o.len() - 1 {
                let inner = s[op + o.len()..cl].trim().trim_matches('"').trim();
                if !inner.is_empty() {
                    return inner.to_string();
                }
            }
        }
    }
    s.trim_matches('"').trim().to_string()
}

/// Tenta ler uma aresta `A -> B` (adjacência) ou `A -->|label| B` (mermaid).
fn parse_edge(l: &str) -> Option<(String, String, Option<String>)> {
    let s = l.replace("-->", "->").replace("-.->", "->").replace("==>", "->");
    let idx = s.find("->")?;
    let left = s[..idx].trim();
    let mut right = s[idx + 2..].trim().to_string();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    let mut label: Option<String> = None;
    // mermaid: right pode vir "|contrato| B"
    if right.starts_with('|') {
        if let Some(end) = right[1..].find('|') {
            label = Some(right[1..1 + end].trim().to_string());
            right = right[1 + end + 1..].trim().to_string();
        }
    }
    // adjacência: "B (contrato)"
    if right.ends_with(')') {
        if let Some(op) = right.rfind('(') {
            label = Some(right[op + 1..right.len() - 1].trim().to_string());
            right = right[..op].trim().to_string();
        }
    }
    let a = clean_node(left);
    let b = clean_node(&right);
    if a.is_empty() || b.is_empty() || a.len() > 48 || b.len() > 48 {
        return None;
    }
    // `|` = célula de tabela vazando um "->" interno (ex.: "api->db"); não é aresta de grafo.
    if a.contains('|') || b.contains('|') {
        return None;
    }
    // nós são identificadores/serviços curtos; frase com muitos espaços é legenda, não nó.
    let spaces = |s: &str| s.matches(' ').count();
    if spaces(&a) > 3 || spaces(&b) > 3 {
        return None;
    }
    Some((a, b, label.filter(|s| !s.is_empty())))
}

/// True se a célula parece `caminho/arquivo.ext:123`.
fn looks_like_loc(c: &str) -> bool {
    if let Some(idx) = c.rfind(':') {
        let after = &c[idx + 1..];
        return !after.is_empty()
            && after.chars().all(|ch| ch.is_ascii_digit())
            && (c.contains('/') || c.contains('.'));
    }
    false
}

/// Lê uma linha de tabela de índice `nome | ... | arquivo:linha` → (nome, loc).
fn parse_func_row(l: &str) -> Option<(String, String)> {
    if !l.contains('|') {
        return None;
    }
    let cells: Vec<&str> = l.trim().trim_matches('|').split('|').map(|c| c.trim()).collect();
    if cells.len() < 2 {
        return None;
    }
    let name = cells[0].trim_matches('`').trim();
    if name.is_empty()
        || name.len() > 80
        || name.eq_ignore_ascii_case("função")
        || name.eq_ignore_ascii_case("funcao")
        || name.starts_with("---")
        || name.starts_with(":--")
    {
        return None;
    }
    let loc = cells.iter().find(|c| looks_like_loc(c))?;
    Some((name.to_string(), loc.trim().to_string()))
}

/// Varre os `.md` do dir do index e monta (nós, arestas).
pub fn parse_graph(dir: &Path) -> (Vec<Node>, Vec<Edge>) {
    let mut locs: BTreeMap<String, String> = BTreeMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut seen_edge: BTreeSet<(String, String)> = BTreeSet::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let text = fs::read_to_string(&p).unwrap_or_default();
            for line in text.lines() {
                let l = line.trim();
                // Arestas: linhas de adjacência/mermaid — nunca linhas de tabela (`|`).
                if l.contains("->") && !l.starts_with('|') {
                    if let Some((a, b, lab)) = parse_edge(l) {
                        ids.insert(a.clone());
                        ids.insert(b.clone());
                        if seen_edge.insert((a.clone(), b.clone())) {
                            edges.push(Edge { from: a, to: b, label: lab });
                        }
                    }
                }
                if l.contains('|') {
                    if let Some((name, loc)) = parse_func_row(l) {
                        ids.insert(name.clone());
                        locs.insert(name, loc);
                    }
                }
            }
        }
    }
    let nodes = ids
        .into_iter()
        .map(|id| {
            let loc = locs.get(&id).cloned();
            Node { id, loc }
        })
        .collect();
    (nodes, edges)
}

// ---------------------------------------------------------------------------
// Estado do overdev (lido direto dos arquivos do control-plane).
// ---------------------------------------------------------------------------

/// Lê o estado do overdev de `root` (objetivo, mode, itens, decisões, plano, perguntas).
/// Consumido pela GUI (view nativa) e pelo HTML. Vazio/`inativo` se não houver run.
pub fn load_overdev(root: &Path) -> Overdev {
    let od = root.join(".overdev");
    let state = fs::read_to_string(od.join("state.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let get = |k: &str, d: &str| {
        state
            .as_ref()
            .and_then(|v| v.get(k))
            .and_then(|v| v.as_str())
            .unwrap_or(d)
            .to_string()
    };
    let mut items = Vec::new();
    let cl = fs::read_to_string(od.join("CHECKLIST.md")).unwrap_or_default();
    for line in cl.lines() {
        let t = line.trim_start();
        let (st, rest) = if let Some(r) = t.strip_prefix("- [ ]") {
            (' ', r)
        } else if let Some(r) = t.strip_prefix("- [x]").or_else(|| t.strip_prefix("- [X]")) {
            ('x', r)
        } else if let Some(r) = t.strip_prefix("- [~]") {
            ('~', r)
        } else {
            continue;
        };
        items.push((st, rest.trim().to_string()));
    }
    Overdev {
        objetivo: get("objetivo", ""),
        mode: get("mode", "inativo"),
        items,
        decisoes: fs::read_to_string(od.join("DECISOES.md")).unwrap_or_default(),
        plano: fs::read_to_string(od.join("PLAN.md")).unwrap_or_default(),
        perguntas: fs::read_to_string(root.join("PERGUNTAS-OVERDEV.txt")).unwrap_or_default(),
    }
}

/// Carrega o grafo do index de `root`: (nós, arestas, dir do index se achou).
pub fn load_graph(root: &Path) -> (Vec<Node>, Vec<Edge>, Option<PathBuf>) {
    match find_index_dir(root) {
        Some(dir) => {
            let (n, e) = parse_graph(&dir);
            (n, e, Some(dir))
        }
        None => (Vec::new(), Vec::new(), None),
    }
}

// ---------------------------------------------------------------------------
// `schematize panel` — gera o HTML e abre no browser.
// ---------------------------------------------------------------------------

const TEMPLATE: &str = include_str!("panel.html");

/// Monta o HTML self-contained do painel de `root`: (html, nº nós, nº arestas, dir index).
pub fn render_html(root: &Path) -> (String, usize, usize, Option<PathBuf>) {
    let ov = load_overdev(root);
    let (o, d, h) = ov.counts();
    let items_json: Vec<serde_json::Value> =
        ov.items.iter().map(|(s, t)| serde_json::json!({ "s": s.to_string(), "t": t })).collect();
    let (nodes, edges, idx) = load_graph(root);
    let nodes_json: Vec<serde_json::Value> =
        nodes.iter().map(|n| serde_json::json!({ "id": n.id, "loc": n.loc })).collect();
    let edges_json: Vec<serde_json::Value> = edges
        .iter()
        .map(|e| serde_json::json!({ "from": e.from, "to": e.to, "label": e.label }))
        .collect();
    let cwd = fs::canonicalize(root)
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let data = serde_json::json!({
        "objetivo": ov.objetivo, "mode": ov.mode,
        "counts": { "open": o, "done": d, "hold": h },
        "items": items_json,
        "decisoes": ov.decisoes, "plano": ov.plano, "perguntas": ov.perguntas,
        "nodes": nodes_json, "edges": edges_json, "cwd": cwd,
        "index": idx.as_ref().map(|p| p.to_string_lossy().into_owned()),
    });
    // `</` escapado pra não fechar o <script> por acidente com conteúdo de MD.
    let blob = data.to_string().replace("</", "<\\/");
    (TEMPLATE.replacen("/*__DATA__*/null", &blob, 1), nodes.len(), edges.len(), idx)
}

/// Gera o HTML de `root` e abre no navegador. Retorna o caminho absoluto do arquivo.
pub fn open_in_browser(root: &Path) -> Result<String, String> {
    let (html, _n, _e, _idx) = render_html(root);
    let out = if root.join(".overdev").is_dir() {
        root.join(".overdev").join("panel.html")
    } else {
        root.join("schematize-panel.html")
    };
    fs::write(&out, html).map_err(|e| e.to_string())?;
    let abs = fs::canonicalize(&out)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| out.to_string_lossy().into_owned());
    open_url(&abs);
    Ok(abs)
}

/// `schematize panel` — painel do diretório atual, aberto no navegador.
pub fn open() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let (_html, n, e, idx) = render_html(&root);
    let abs = open_in_browser(&root)?;
    println!("painel: {abs}");
    println!(
        "  {n} nós / {e} arestas do index{}",
        match &idx {
            Some(d) => format!(" ({})", d.display()),
            None => " (index não encontrado — rode /eng-index)".to_string(),
        }
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// `schematize graph obsidian` — exporta o index como vault Obsidian.
// ---------------------------------------------------------------------------

/// Slug seguro pra nome de arquivo a partir de um id de nó.
fn slug(id: &str) -> String {
    let mut s: String = id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.trim_matches('-').to_string()
}

/// Exporta o grafo do index de `root` como vault Obsidian (uma nota por nó, com [[wikilinks]]).
/// Retorna o diretório do vault criado.
pub fn export_obsidian_at(root: &Path, out: Option<String>) -> Result<PathBuf, String> {
    let idx = find_index_dir(root)
        .ok_or_else(|| "index não encontrado (rode /eng-index primeiro)".to_string())?;
    let (nodes, edges) = parse_graph(&idx);
    if nodes.is_empty() {
        return Err("grafo vazio: nenhum nó parseado do index".to_string());
    }
    let outdir = out.map(PathBuf::from).unwrap_or_else(|| {
        idx.parent().unwrap_or_else(|| Path::new(".")).join("obsidian")
    });
    fs::create_dir_all(&outdir).map_err(|e| e.to_string())?;

    let locs: BTreeMap<&str, &str> =
        nodes.iter().filter_map(|n| n.loc.as_deref().map(|l| (n.id.as_str(), l))).collect();
    let mut outgoing: BTreeMap<&str, Vec<(&str, &Option<String>)>> = BTreeMap::new();
    let mut incoming: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in &edges {
        outgoing.entry(&e.from).or_default().push((&e.to, &e.label));
        incoming.entry(&e.to).or_default().push(&e.from);
    }

    for n in &nodes {
        let mut md = String::new();
        md.push_str("---\n");
        md.push_str(&format!("id: \"{}\"\n", n.id.replace('"', "'")));
        if let Some(l) = &n.loc {
            md.push_str(&format!("loc: \"{l}\"\n"));
        }
        md.push_str("tags: [schematize/index]\n---\n\n");
        md.push_str(&format!("# {}\n\n", n.id));
        if let Some(l) = &n.loc {
            md.push_str(&format!("`{l}`\n\n"));
        }
        if let Some(outs) = outgoing.get(n.id.as_str()) {
            md.push_str("## Chama (out)\n");
            for (to, lab) in outs {
                match lab {
                    Some(x) if !x.is_empty() => md.push_str(&format!("- [[{to}]] — {x}\n")),
                    _ => md.push_str(&format!("- [[{to}]]\n")),
                }
            }
            md.push('\n');
        }
        if let Some(ins) = incoming.get(n.id.as_str()) {
            md.push_str("## É chamado por (in)\n");
            for from in ins {
                md.push_str(&format!("- [[{from}]]\n"));
            }
            md.push('\n');
        }
        let fname = format!("{}.md", slug(&n.id));
        fs::write(outdir.join(fname), md).map_err(|e| e.to_string())?;
    }

    // Hub.
    let mut hub = String::from("---\ntags: [schematize/index]\n---\n\n# MAPA do index\n\n");
    hub.push_str(&format!(
        "> {} nós, {} arestas. Gerado por `schematize graph obsidian` a partir de `{}`.\n> Abra o **Graph View** do Obsidian para navegar.\n\n## Nós\n",
        nodes.len(),
        edges.len(),
        idx.display()
    ));
    for n in &nodes {
        match locs.get(n.id.as_str()) {
            Some(l) => hub.push_str(&format!("- [[{}]] — `{}`\n", n.id, l)),
            None => hub.push_str(&format!("- [[{}]]\n", n.id)),
        }
    }
    fs::write(outdir.join("_MAPA.md"), hub).map_err(|e| e.to_string())?;
    Ok(fs::canonicalize(&outdir).unwrap_or(outdir))
}

/// `schematize graph obsidian` — exporta o index do diretório atual.
pub fn export_obsidian(out: Option<String>) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let dir = export_obsidian_at(&root, out)?;
    println!("vault Obsidian: {}", dir.display());
    println!("  Abra a pasta no Obsidian → Graph View.");
    Ok(())
}
