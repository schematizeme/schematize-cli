//! Exportação do índice como vault Obsidian (uma nota por nó, links `[[...]]`).

use super::*;

/// Slug seguro pra nome de arquivo a partir de um id de nó.
pub(crate) fn slug(id: &str) -> String {
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
