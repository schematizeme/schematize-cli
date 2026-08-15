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
struct Node {
    id: String,
    loc: Option<String>,
}
/// Aresta dirigida do grafo, com rótulo opcional (contrato/rota/evento).
struct Edge {
    from: String,
    to: String,
    label: Option<String>,
}

// ---------------------------------------------------------------------------
// Descoberta do index e parsing do grafo (best-effort, tolerante a formato).
// ---------------------------------------------------------------------------

/// Procura `<algo>_archive/index/` a partir do cwd e do pai. None se não achar.
fn find_index_dir() -> Option<PathBuf> {
    for base in [PathBuf::from("."), PathBuf::from("..")] {
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
    let direct = PathBuf::from("index");
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
fn parse_graph(dir: &Path) -> (Vec<Node>, Vec<Edge>) {
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

/// (objetivo, mode, itens [(estado, texto)]) do run corrente, se houver.
fn read_overdev() -> (String, String, Vec<(char, String)>) {
    let state = fs::read_to_string(".overdev/state.json")
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let objetivo = state
        .as_ref()
        .and_then(|v| v.get("objetivo"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mode = state
        .as_ref()
        .and_then(|v| v.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("inativo")
        .to_string();
    let mut items = Vec::new();
    let cl = fs::read_to_string(".overdev/CHECKLIST.md").unwrap_or_default();
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
    (objetivo, mode, items)
}

// ---------------------------------------------------------------------------
// `schematize panel` — gera o HTML e abre no browser.
// ---------------------------------------------------------------------------

const TEMPLATE: &str = include_str!("panel.html");

/// Monta o painel HTML self-contained e o abre no navegador.
pub fn open() -> Result<(), String> {
    let (objetivo, mode, items) = read_overdev();
    let (mut open_c, mut done_c, mut hold_c) = (0u32, 0u32, 0u32);
    let items_json: Vec<serde_json::Value> = items
        .iter()
        .map(|(s, t)| {
            match s {
                'x' => done_c += 1,
                '~' => hold_c += 1,
                _ => open_c += 1,
            }
            serde_json::json!({ "s": s.to_string(), "t": t })
        })
        .collect();

    let idx = find_index_dir();
    let (nodes, edges) = match &idx {
        Some(d) => parse_graph(d),
        None => (Vec::new(), Vec::new()),
    };
    let nodes_json: Vec<serde_json::Value> =
        nodes.iter().map(|n| serde_json::json!({ "id": n.id, "loc": n.loc })).collect();
    let edges_json: Vec<serde_json::Value> = edges
        .iter()
        .map(|e| serde_json::json!({ "from": e.from, "to": e.to, "label": e.label }))
        .collect();

    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_default();
    let data = serde_json::json!({
        "objetivo": objetivo,
        "mode": mode,
        "counts": { "open": open_c, "done": done_c, "hold": hold_c },
        "items": items_json,
        "decisoes": fs::read_to_string(".overdev/DECISOES.md").unwrap_or_default(),
        "plano": fs::read_to_string(".overdev/PLAN.md").unwrap_or_default(),
        "perguntas": fs::read_to_string("PERGUNTAS-OVERDEV.txt").unwrap_or_default(),
        "nodes": nodes_json,
        "edges": edges_json,
        "cwd": cwd,
        "index": idx.as_ref().map(|p| p.to_string_lossy().into_owned()),
    });
    // `</` escapado pra não fechar o <script> por acidente com conteúdo de MD.
    let blob = data.to_string().replace("</", "<\\/");
    let html = TEMPLATE.replacen("/*__DATA__*/null", &blob, 1);

    let out = if Path::new(".overdev").is_dir() {
        PathBuf::from(".overdev/panel.html")
    } else {
        PathBuf::from("schematize-panel.html")
    };
    fs::write(&out, html).map_err(|e| e.to_string())?;
    let abs = fs::canonicalize(&out)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| out.to_string_lossy().into_owned());
    println!("painel: {abs}");
    println!(
        "  {} nós / {} arestas do index{}",
        nodes.len(),
        edges.len(),
        match &idx {
            Some(d) => format!(" ({})", d.display()),
            None => " (index não encontrado — rode /eng-index)".to_string(),
        }
    );
    open_url(&abs);
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

/// Exporta o grafo do index como vault Obsidian (uma nota por nó, com [[wikilinks]]).
pub fn export_obsidian(out: Option<String>) -> Result<(), String> {
    let idx = find_index_dir()
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

    let abs = fs::canonicalize(&outdir)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| outdir.to_string_lossy().into_owned());
    println!("vault Obsidian: {abs}");
    println!("  {} notas ({} nós, {} arestas). Abra a pasta no Obsidian → Graph View.", nodes.len() + 1, nodes.len(), edges.len());
    Ok(())
}
