//! Painel auxiliar (HTML no browser) + export Obsidian do grafo do index.
//! O quê: `schematize panel` gera um HTML self-contained (sem CDN) com o estado do
//! overdev (objetivo, checklist, decisões, plano, perguntas parkeadas) e um grafo
//! force-directed estilo Obsidian do index (nós linkados a arquivo:linha); abre no
//! browser. `schematize graph obsidian` exporta o index como vault Obsidian
//! (markdown + [[wikilinks]]). Onde: chamado por main; lê `.overdev/*` e
//! `<projeto>_archive/index/*.md`. É read-mostly — o juiz do "terminou" segue sendo o
//! checklist+gate (overdev.rs); o painel só dá visão.

use crate::util::open_url;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// Descobre o dir do grafo/index de `root`, por ORDEM DE PRIORIDADE:
/// 1) `<root>/.schematize/grafos/` — o local OPERACIONAL vivo (grafo global + por serviço);
/// 2) `<algo>_archive/index/` sob `root` ou seu pai — o espelho durável (layout §39 clássico);
/// 3) `<root>/index/` direto. None se nada existir.
pub fn find_index_dir(root: &Path) -> Option<PathBuf> {
    // (1) dir operacional novo tem prioridade — é onde o reindex grava a versão viva.
    let grafos = crate::paths::grafos_dir_at(root);
    if grafos.is_dir() {
        return Some(grafos);
    }
    // (2) espelho no archive (do próprio root ou do pai, p/ o caso projeto-dentro-de-umbrella).
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
    // (3) fallback: `<root>/index/`.
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
    // Normaliza toda variante de seta pra `->`: mermaid (`-->`,`-.->`,`==>`) E unicode (`→`,`⟶`,`⇒`),
    // que o índice às vezes grava na lista de adjacência pesquisável e que o parser antes ignorava.
    let s = l
        .replace("-->", "->")
        .replace("-.->", "->")
        .replace("==>", "->")
        .replace('→', "->")
        .replace('⟶', "->")
        .replace('⇒', "->");
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

/// Lê uma microfunção em BULLET do MAPA/§39: `- \`nome\` — descrição · arquivo:linha` (ou sem loc).
/// Devolve `(nome, Option<loc>)`. None se não for um bullet com nome curto e um em-dash de descrição.
/// NUNCA trata aresta (`->`) como função. É o que faz as microfunções em lista virarem nós do grafo.
fn parse_func_bullet(l: &str) -> Option<(String, Option<String>)> {
    // Precisa ser bullet (`-`/`*`) — evita casar linhas de texto corrido.
    let t = l.trim_start();
    if !(t.starts_with("- ") || t.starts_with("* ")) {
        return None;
    }
    let s = t[2..].trim();
    if s.is_empty() || s.contains("->") {
        return None;
    }
    // Precisa do em-dash separando nome — descrição (mesma convenção de `parse_desc_line`).
    let i = s.find('—')?;
    let name = clean_node(s[..i].trim());
    if name.is_empty() || name.len() > 80 || name.matches(' ').count() > 4 {
        return None;
    }
    // Localização opcional: a última célula que parecer `arquivo.ext:linha` (após `·`, `—` ou espaço).
    let rest = &s[i + '—'.len_utf8()..];
    let loc = rest
        .split(['·', '—', '|', '(', ')'])
        .flat_map(|seg| seg.split_whitespace())
        .map(|c| c.trim_matches('`').trim())
        .find(|c| looks_like_loc(c))
        .map(|c| c.to_string());
    Some((name, loc))
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

/// Varre TODOS os `.md` do dir do index e monta (nós, arestas) — funde tudo (visão legada).
pub fn parse_graph(dir: &Path) -> (Vec<Node>, Vec<Edge>) {
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("md") {
                files.push(p);
            }
        }
    }
    files.sort();
    parse_graph_files(&files)
}

/// Monta (nós, arestas) parseando SÓ os arquivos dados (ex.: apenas `GRAFO_GLOBAL.md`, ou um
/// `<servico>.md`). Base compartilhada por [`parse_graph`], [`load_graph`] e [`load_service_graph`].
pub fn parse_graph_files(files: &[PathBuf]) -> (Vec<Node>, Vec<Edge>) {
    let mut locs: BTreeMap<String, String> = BTreeMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut seen_edge: BTreeSet<(String, String)> = BTreeSet::new();
    {
        for p in files {
            let text = fs::read_to_string(p).unwrap_or_default();
            for line in text.lines() {
                let l = line.trim();
                // Arestas: linhas de adjacência/mermaid (ASCII `->`/`-->` OU unicode `→`/`⟶`/`⇒`) —
                // nunca linhas de tabela (`|`).
                let has_arrow = l.contains("->") || l.contains('→') || l.contains('⟶') || l.contains('⇒');
                if has_arrow && !l.starts_with('|') {
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
                } else if let Some((name, loc)) = parse_func_bullet(l) {
                    // Microfunção em bullet `- \`nome\` — desc · arquivo:linha` (formato comum do
                    // MAPA §39) — vira nó com localização, senão essas funções nunca apareciam.
                    ids.insert(name.clone());
                    if let Some(loc) = loc {
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

/// Lê uma linha de tabela de índice e extrai (nó, descrição curta): o nome é a 1ª
/// célula (mesma normalização de `parse_func_row`), a descrição é a 1ª célula
/// seguinte que NÃO é um `arquivo:linha`, separador ou vazia (tipicamente a coluna
/// "O quê"). Best-effort: None se não parecer uma linha de dado.
fn parse_desc_row(l: &str) -> Option<(String, String)> {
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
        || name.starts_with("---")
        || name.starts_with(":--")
        || name.eq_ignore_ascii_case("função")
        || name.eq_ignore_ascii_case("funcao")
        || name.eq_ignore_ascii_case("repo/serviço")
        || name.eq_ignore_ascii_case("origem")
        || name.eq_ignore_ascii_case("pasta top-level")
    {
        return None;
    }
    let desc = cells[1..]
        .iter()
        .map(|c| c.trim_matches('`').trim())
        .find(|c| !c.is_empty() && !looks_like_loc(c) && !c.starts_with("---") && !c.starts_with(":--"))?;
    if desc.is_empty() || desc.len() > 240 {
        return None;
    }
    Some((name.to_string(), desc.to_string()))
}

/// Lê uma linha de doc/hub `nó — descrição` (aceita bullet e `[[nó]]`); usa o
/// em-dash como separador. None se não casar. Nunca trata aresta (`->`) como desc.
fn parse_desc_line(l: &str) -> Option<(String, String)> {
    let s = l.trim_start_matches(['-', '*', '>']).trim();
    if s.is_empty() || s.contains("->") {
        return None;
    }
    let i = s.find('—')?;
    let left = s[..i].trim();
    let right = s[i + '—'.len_utf8()..].trim().trim_matches('`').trim();
    if left.is_empty() || right.is_empty() || right.len() > 240 {
        return None;
    }
    let name = clean_node(left);
    if name.is_empty() || name.len() > 80 {
        return None;
    }
    Some((name, right.to_string()))
}

/// Descrição curta por NÓ do índice (mesma normalização de `parse_graph`): parseia
/// o MAPA/índice em `<projeto>_archive/index/` e extrai, best-effort, a linha/descrição
/// daquele nó — da coluna "O quê" de uma tabela ou de uma linha `nó — descrição`.
/// Nó sem descrição fica ausente. Usado pelo clique no nó do grafo (bloco de texto).
pub fn node_descriptions(root: &Path) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let Some(dir) = find_index_dir(root) else {
        return out;
    };
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let text = fs::read_to_string(&p).unwrap_or_default();
            for line in text.lines() {
                let l = line.trim();
                if l.starts_with('|') {
                    if let Some((name, desc)) = parse_desc_row(l) {
                        out.entry(name).or_insert(desc);
                    }
                } else if let Some((name, desc)) = parse_desc_line(l) {
                    out.entry(name).or_insert(desc);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Estado do overdev (lido direto dos arquivos do control-plane).
// ---------------------------------------------------------------------------

/// Lê o estado do overdev de `root` (objetivo, mode, itens, decisões, plano, perguntas).
/// Consumido pela GUI (view nativa) e pelo HTML. Vazio/`inativo` se não houver run.
pub fn load_overdev(root: &Path) -> Overdev {
    let od = crate::paths::overdev_dir_at(root);
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
///
/// Se existir um `GRAFO_GLOBAL.md` no dir (layout novo `.schematize/grafos/`), parseia SÓ ele —
/// a visão GLOBAL limpa da aplicação (serviços + funções principais + contratos), sem fundir os
/// grafos por-serviço num amontoado (o que deixava o grafo "zoneado"). Sem o arquivo global,
/// cai no comportamento legado: funde todos os `.md` do dir.
pub fn load_graph(root: &Path) -> (Vec<Node>, Vec<Edge>, Option<PathBuf>) {
    match find_index_dir(root) {
        Some(dir) => {
            let global = dir.join("GRAFO_GLOBAL.md");
            let (n, e) = if global.is_file() {
                parse_graph_files(&[global])
            } else {
                parse_graph(&dir)
            };
            (n, e, Some(dir))
        }
        None => (Vec::new(), Vec::new(), None),
    }
}

/// Carrega o grafo DETALHADO de UM microserviço `<servico>.md` do dir de grafos de `root`
/// (drill-down do grafo global). `(nós, arestas)` vazio se o arquivo não existir.
pub fn load_service_graph(root: &Path, servico: &str) -> (Vec<Node>, Vec<Edge>) {
    let Some(dir) = find_index_dir(root) else {
        return (Vec::new(), Vec::new());
    };
    // sanitiza pra basename (anti path-traversal) e casa `<servico>.md`.
    let base = Path::new(servico).file_name().and_then(|s| s.to_str()).unwrap_or(servico);
    let f = dir.join(format!("{base}.md"));
    if f.is_file() {
        parse_graph_files(&[f])
    } else {
        (Vec::new(), Vec::new())
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
    let od = crate::paths::overdev_dir_at(root);
    let out = if od.is_dir() {
        od.join("panel.html")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Setas UNICODE (`→`) na adjacência viram arestas — antes o parser só via ASCII `->`/`-->`.
    #[test]
    fn parse_edge_aceita_seta_unicode() {
        let (a, b, lab) = parse_edge("front → api (login)").expect("aresta unicode");
        assert_eq!((a.as_str(), b.as_str()), ("front", "api"));
        assert_eq!(lab.as_deref(), Some("login"));
        // ASCII segue funcionando.
        let (c, d, _) = parse_edge("api -> db").expect("aresta ascii");
        assert_eq!((c.as_str(), d.as_str()), ("api", "db"));
    }

    /// Microfunção em BULLET vira nó com localização (`- \`nome\` — desc · arquivo:linha`).
    #[test]
    fn parse_func_bullet_extrai_no_e_loc() {
        let (n, loc) = parse_func_bullet("- `login` — autentica o usuário · auth.rs:42").expect("bullet");
        assert_eq!(n, "login");
        assert_eq!(loc.as_deref(), Some("auth.rs:42"));
        // Sem loc: nó sem localização.
        let (n2, loc2) = parse_func_bullet("- `logout` — encerra a sessão").expect("bullet sem loc");
        assert_eq!(n2, "logout");
        assert!(loc2.is_none());
        // Aresta não é função.
        assert!(parse_func_bullet("- api -> db").is_none());
        // Linha sem bullet não casa.
        assert!(parse_func_bullet("login — algo").is_none());
    }

    /// `load_graph` prefere `GRAFO_GLOBAL.md` (visão global limpa) e ignora os demais `.md` do dir.
    #[test]
    fn load_graph_prioriza_grafo_global() {
        let root = std::env::temp_dir().join(format!("schz-graph-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let grafos = root.join(".schematize/grafos");
        fs::create_dir_all(&grafos).unwrap();
        // Global: só a fronteira entre serviços.
        fs::write(grafos.join("GRAFO_GLOBAL.md"), "front -> api (login)\napi -> auth (token)\n").unwrap();
        // Detalhe por serviço: NÃO deve entrar na visão global.
        fs::write(grafos.join("api.md"), "handler -> repo\nrepo -> pg\n").unwrap();

        let (nodes, edges, dir) = load_graph(&root);
        assert!(dir.is_some());
        let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains("front") && ids.contains("api") && ids.contains("auth"));
        assert!(!ids.contains("repo") && !ids.contains("pg"), "não pode fundir o grafo por-serviço no global");
        assert_eq!(edges.len(), 2);

        // Drill-down: o grafo detalhado do serviço é acessível à parte.
        let (snodes, _sedges) = load_service_graph(&root, "api");
        let sids: std::collections::HashSet<&str> = snodes.iter().map(|n| n.id.as_str()).collect();
        assert!(sids.contains("repo") && sids.contains("pg"));

        let _ = fs::remove_dir_all(&root);
    }

    /// `find_index_dir` prioriza `.schematize/grafos/` sobre o `_archive/index/` legado.
    #[test]
    fn find_index_dir_prefere_schematize_grafos() {
        let root = std::env::temp_dir().join(format!("schz-idxdir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".schematize/grafos")).unwrap();
        fs::create_dir_all(root.join("proj_archive/index")).unwrap();
        assert_eq!(find_index_dir(&root), Some(root.join(".schematize/grafos")));
        let _ = fs::remove_dir_all(&root);
    }
}
