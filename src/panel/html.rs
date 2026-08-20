//! `schematize panel` — renderiza o painel HTML self-contained e abre no browser.

use super::*;

pub(crate) const TEMPLATE: &str = include_str!("panel.html");

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
