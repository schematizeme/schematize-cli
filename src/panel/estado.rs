//! Leitura do estado do overdev de um projeto (objetivo, modo, itens, seções).

use super::*;

/// Lê o estado do overdev de `root` (objetivo, mode, itens, decisões, plano, perguntas).
/// Consumido pela GUI (view nativa) e pelo HTML. Vazio/`inativo` se não houver run.
pub fn load_overdev(root: &Path) -> Overdev {
    let od = crate::paths::overdev_dir_at(root);
    let state = fs::read_to_string(od.join("state.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let get = |k: &str, d: &str| {
        state.as_ref().and_then(|v| v.get(k)).and_then(|v| v.as_str()).unwrap_or(d).to_string()
    };
    let mut items = Vec::new();
    // Checklist pode ser 1 arquivo (CHECKLIST.md) OU a pasta checklist/ com vários .md (granularidade
    // / split multiagent) — lê e concatena todos.
    let cl = crate::paths::read_multidoc(&od, "CHECKLIST.md", "checklist");
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
        decisoes: crate::paths::read_multidoc(&od, "DECISOES.md", "decisoes"),
        plano: crate::paths::read_multidoc(&od, "PLAN.md", "plan"),
        perguntas: fs::read_to_string(root.join("PERGUNTAS-OVERDEV.txt")).unwrap_or_default(),
    }
}
