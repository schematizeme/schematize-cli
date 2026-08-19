//! guiactions — COMPATIBILIDADE skill → GUI: qualquer skill instalada pode declarar botões que a GUI
//! renderiza, sem hardcode. O quê: lê `~/.claude/skills/<skill>/gui.json` de cada skill INSTALADA e
//! junta as ações num só vetor. Onde: a GUI chama `gui_actions()` e desenha um botão por ação; clicar
//! dispara `command` (ex.: `/eng-qa`, `/pentest-authz`) no `claude` de um terminal externo. É como
//! Q.A. e Pentest aparecem quando as skills estão instaladas — e como skills novas plugam na GUI.
//!
//! Formato do `gui.json` (na raiz da skill):
//! ```json
//! { "actions": [
//!   { "label": "Q.A.", "command": "/eng-qa", "needs_project": true, "context": "project", "order": 10 }
//! ] }
//! ```
//! `needs_project` (default false): o botão só habilita com um projeto selecionado. `context` (default
//! "project"): onde aparece ("project" = aba Overdev/projeto; "global" = sempre). `order`: ordenação.

use serde::Deserialize;

/// Uma ação declarada por uma skill pra virar botão na GUI.
#[derive(Clone, Debug, Deserialize)]
pub struct GuiAction {
    /// Rótulo do botão (ex.: "Q.A.", "Pentest").
    pub label: String,
    /// Comando disparado no `claude` (ex.: "/eng-qa"). Pode ser um slash-command ou um prompt.
    pub command: String,
    /// Precisa de um projeto selecionado pra habilitar.
    #[serde(default)]
    pub needs_project: bool,
    /// Onde aparece: "project" (aba do projeto) | "global" (sempre). Default "project".
    #[serde(default = "default_context")]
    pub context: String,
    /// Ordenação (menor primeiro).
    #[serde(default)]
    pub order: i64,
    /// Slug da skill dona (preenchido no carregamento; não vem do JSON).
    #[serde(skip)]
    pub skill: String,
}

fn default_context() -> String {
    "project".into()
}

#[derive(Deserialize)]
struct GuiManifest {
    #[serde(default)]
    actions: Vec<GuiAction>,
}

/// Junta as ações de GUI de TODAS as skills instaladas (que tenham `gui.json`). Offline: varre
/// `~/.claude/skills/` direto, sem catálogo remoto. Ordenado por `order` e depois `label`.
pub fn gui_actions() -> Vec<GuiAction> {
    let dir = crate::util::skills_dir();
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        // Skill INSTALADA = pasta com VERSION. Sem gui.json → a skill não declara ações.
        if !p.is_dir() || !p.join("VERSION").is_file() {
            continue;
        }
        let Ok(txt) = std::fs::read_to_string(p.join("gui.json")) else { continue };
        let Ok(m) = serde_json::from_str::<GuiManifest>(&txt) else { continue };
        let slug = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .trim_start_matches("schematize-")
            .to_string();
        for mut a in m.actions {
            a.skill = slug.clone();
            out.push(a);
        }
    }
    out.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.label.cmp(&b.label)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parseia_manifesto_com_defaults() {
        let m: GuiManifest = serde_json::from_str(
            r#"{ "actions": [
                { "label": "Q.A.", "command": "/eng-qa", "needs_project": true, "order": 10 },
                { "label": "Pentest", "command": "/pentest-authz" }
            ] }"#,
        )
        .unwrap();
        assert_eq!(m.actions.len(), 2);
        assert!(m.actions[0].needs_project);
        assert_eq!(m.actions[0].context, "project"); // default
        assert!(!m.actions[1].needs_project); // default false
        assert_eq!(m.actions[1].command, "/pentest-authz");
    }
}
