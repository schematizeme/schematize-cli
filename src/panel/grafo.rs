//! Montagem das VISÕES do grafo: onde mora o índice, a visão global (com cap e
//! agregação por microserviço), o drill por serviço e as descrições por nó.

use super::*;

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
/// (drill-down do grafo global). Se não houver um `<servico>.md` autorado, cai no FALLBACK: filtra o
/// índice flat aos nós DESSE serviço (loc começando com `<servico>/`) — assim o drill funciona mesmo
/// sem grafo por-serviço escrito à mão. `(nós, arestas)` vazio se não achar nada.
pub fn load_service_graph(root: &Path, servico: &str) -> (Vec<Node>, Vec<Edge>) {
    let Some(dir) = find_index_dir(root) else {
        return (Vec::new(), Vec::new());
    };
    // O id do nó agregado é "<serviço> · N" (com contagem de funções). Tira o sufixo pra recuperar
    // o nome do serviço; depois sanitiza a basename (anti path-traversal).
    let name = strip_count_suffix(servico);
    let base = Path::new(name).file_name().and_then(|s| s.to_str()).unwrap_or(name);
    let f = dir.join(format!("{base}.md"));
    if f.is_file() {
        return parse_graph_files(&[f]);
    }
    // Fallback: subgrafo do serviço a partir do índice flat.
    let (nodes, edges) = parse_graph(&dir);
    service_subgraph(&nodes, &edges, base)
}

/// Remove o sufixo " · N" (contagem) que a agregação põe no id do nó de serviço → nome do serviço.
/// Só remove se o que vem depois de " · " for tudo dígito (senão devolve intacto).
pub(crate) fn strip_count_suffix(s: &str) -> &str {
    match s.rsplit_once(" · ") {
        Some((svc, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => svc,
        _ => s,
    }
}

/// Cap de nós do grafo GLOBAL: acima disso, um índice flat vira "amontoado" ilegível (o usuário
/// relatou 1600+ nós em `owiew`). Passou disso e sem `GRAFO_GLOBAL.md` autorado → agrega por serviço.
pub const GLOBAL_NODE_CAP: usize = 60;

/// Microserviço de um nó = 1º componente do path do `loc` (`<servico>/…:linha`). `None` se o nó não
/// tem loc ou o loc não tem diretório (nó de raiz). Base da agregação e do subgrafo de drill.
pub(crate) fn service_of(loc: &Option<String>) -> Option<String> {
    let loc = loc.as_deref()?;
    let path = loc.rsplit_once(':').map(|(p, _)| p).unwrap_or(loc);
    let first = path.split(['/', '\\']).next()?;
    if first.is_empty() || first == path {
        None // sem diretório → nó na raiz do projeto, não é microserviço
    } else {
        Some(first.to_string())
    }
}

/// Agrega um grafo flat em UM nó por microserviço (id = nome do serviço + contagem de funções);
/// arestas viram serviço→serviço (dedup, sem self-loop). Nós sem serviço identificável caem num
/// balde `(raiz)`. É o que transforma 1600 nós num mapa mental de ~N serviços.
pub fn aggregate_by_service(nodes: &[Node], edges: &[Edge]) -> (Vec<Node>, Vec<Edge>) {
    use std::collections::BTreeMap;
    let name_svc: std::collections::HashMap<&str, String> = nodes
        .iter()
        .map(|n| (n.id.as_str(), service_of(&n.loc).unwrap_or_else(|| "(raiz)".into())))
        .collect();
    // contagem por serviço → rótulo "<serviço> · N".
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for n in nodes {
        *counts.entry(name_svc.get(n.id.as_str()).cloned().unwrap_or_else(|| "(raiz)".into())).or_default() += 1;
    }
    let agg_nodes: Vec<Node> = counts
        .iter()
        .map(|(svc, n)| Node { id: format!("{svc} · {n}"), loc: None })
        .collect();
    // mapa serviço → id-rotulado (pra as arestas casarem os nós agregados).
    let svc_label: BTreeMap<&str, String> =
        counts.iter().map(|(svc, n)| (svc.as_str(), format!("{svc} · {n}"))).collect();
    let mut seen: std::collections::BTreeSet<(String, String)> = Default::default();
    let mut agg_edges = Vec::new();
    for e in edges {
        let (Some(sa), Some(sb)) = (name_svc.get(e.from.as_str()), name_svc.get(e.to.as_str())) else {
            continue;
        };
        if sa == sb {
            continue; // aresta interna ao serviço não aparece no global
        }
        let (la, lb) = (svc_label[sa.as_str()].clone(), svc_label[sb.as_str()].clone());
        if seen.insert((la.clone(), lb.clone())) {
            agg_edges.push(Edge { from: la, to: lb, label: None });
        }
    }
    (agg_nodes, agg_edges)
}

/// Subgrafo de um serviço: só os nós cujo `loc` começa com `<servico>/` (+ arestas entre eles).
pub(crate) fn service_subgraph(nodes: &[Node], edges: &[Edge], servico: &str) -> (Vec<Node>, Vec<Edge>) {
    let keep: std::collections::HashSet<&str> = nodes
        .iter()
        .filter(|n| service_of(&n.loc).as_deref() == Some(servico))
        .map(|n| n.id.as_str())
        .collect();
    let sub_nodes: Vec<Node> = nodes.iter().filter(|n| keep.contains(n.id.as_str())).cloned().collect();
    let sub_edges: Vec<Edge> = edges
        .iter()
        .filter(|e| keep.contains(e.from.as_str()) && keep.contains(e.to.as_str()))
        .cloned()
        .collect();
    (sub_nodes, sub_edges)
}

/// Grafo GLOBAL "paginado" — a visão de entrada, sempre legível:
/// 1) `GRAFO_GLOBAL.md` autorado, se existir (visão curada);
/// 2) senão, índice flat; se ele passar de [`GLOBAL_NODE_CAP`] nós, AGREGA por microserviço
///    (1 nó por serviço) — o drill (`load_service_graph`) abre o detalhe;
/// 3) senão, o flat mesmo (projeto pequeno cabe na tela).
/// Retorna `(nós, arestas, dir-do-index, agregado?)`.
pub fn load_graph_global(root: &Path) -> (Vec<Node>, Vec<Edge>, Option<PathBuf>, bool) {
    let Some(dir) = find_index_dir(root) else {
        return (Vec::new(), Vec::new(), None, false);
    };
    let global = dir.join("GRAFO_GLOBAL.md");
    // Origem preferida: o global AUTORADO; senão o índice flat da pasta.
    let (n, e) = if global.is_file() { parse_graph_files(&[global]) } else { parse_graph(&dir) };
    // O CAP vale pra QUALQUER origem. O `GRAFO_GLOBAL.md` autorado costuma trazer,
    // além dos nós de serviço, as TABELAS de funções de cada repo — num projeto de
    // verdade isso passa de 600 nós e a "visão de entrada" deixa de ser legível (e a
    // física do app passa a simular 600 corpos por quadro). Antes essa origem
    // pulava o cap, e era justamente ela que o app carregava ao abrir o projeto.
    if n.len() > GLOBAL_NODE_CAP {
        let (an, ae) = aggregate_by_service(&n, &e);
        (an, ae, Some(dir), true)
    } else {
        (n, e, Some(dir), false)
    }
}
