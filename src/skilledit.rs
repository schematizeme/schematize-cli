//! Criação e edição de skills instaladas — o backend de "autoria" da casa.
//! O quê: scaffolda o piso mínimo VÁLIDO de uma skill em `~/.claude/skills/schematize-<slug>/`
//! e lê/lista/grava os arquivos editáveis dela com segurança de caminho (deny-by-default).
//! Onde: chamado pela CLI (`schematize skills new/edit`) e, futuramente, pela GUI (Slint).
//! Toda escrita fica confinada à raiz da skill — nada escapa via `..`/caminho absoluto/symlink.

use crate::util::skills_dir;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Versão inicial de toda skill nova (piso mínimo válido da casa).
const INITIAL_VERSION: &str = "0.1.0";

// ------------------------------------------------------------------------------------------------
// Validação de slug e de caminho (deny-by-default, mesma postura do `sshkeys::valid_name`).
// ------------------------------------------------------------------------------------------------

/// Valida o slug por ALLOW-LIST: só `[a-z0-9-]`, começa por letra/dígito, sem `/`, sem `..`,
/// sem vazio e sem barra invertida. Deny-by-default — o que não casa a regra é recusado.
/// Fluxo: chamado antes de montar qualquer caminho a partir do slug (scaffold e resolução da raiz).
pub fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() || slug.len() > 64 {
        return Err(format!("slug inválido (vazio ou >64): {slug:?}"));
    }
    let first = slug.chars().next().unwrap();
    if !first.is_ascii_alphanumeric() {
        return Err(format!("slug deve começar por letra/dígito: {slug:?}"));
    }
    if !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!("slug só aceita [a-z0-9-]: {slug:?}"));
    }
    if slug.contains("..") || slug.ends_with('-') {
        return Err(format!("slug inválido (`..` ou termina em `-`): {slug:?}"));
    }
    Ok(())
}

/// Raiz da skill instalada a partir do slug: `~/.claude/skills/schematize-<slug>/`.
/// Valida o slug antes — nunca monta caminho a partir de slug não sanitizado.
pub fn skill_root(slug: &str) -> Result<PathBuf, String> {
    validate_slug(slug)?;
    Ok(skills_dir().join(format!("schematize-{slug}")))
}

/// Normaliza um caminho RELATIVO recusando tudo que possa escapar: vazio, absoluto,
/// componente `..` (ParentDir) ou raiz/prefixo. Só componentes `Normal` (e `.` ignorado)
/// passam. Retorna o caminho relativo já limpo (sem `.`). Sem tocar no filesystem.
fn safe_rel(rel: &str) -> Result<PathBuf, String> {
    if rel.trim().is_empty() {
        return Err("caminho vazio".to_string());
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(format!("caminho absoluto recusado: {rel:?}"));
    }
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            // ParentDir (`..`), RootDir e Prefix são traversal — recusados.
            _ => return Err(format!("caminho inválido (traversal): {rel:?}")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(format!("caminho vazio após normalização: {rel:?}"));
    }
    Ok(out)
}

/// Resolve `rel` DENTRO da raiz `root`, com dupla trava: (1) `safe_rel` rejeita `..`/absoluto
/// no plano lógico; (2) canonicaliza a raiz e o ancestral EXISTENTE do candidato e confere o
/// prefixo — isso derruba também escapes via symlink. Retorna o caminho final (pode não existir).
fn resolve_within(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let safe = safe_rel(rel)?;
    let candidate = root.join(&safe);

    // A raiz da skill tem que existir pra canonicalizar (a skill precisa estar instalada).
    let croot = root
        .canonicalize()
        .map_err(|e| format!("raiz da skill inacessível: {e}"))?;

    // Sobe até o primeiro ancestral que existe (o arquivo alvo pode ainda não existir num write).
    let mut existing = candidate.clone();
    while !existing.exists() {
        match existing.parent() {
            Some(p) => existing = p.to_path_buf(),
            None => break,
        }
    }
    if let Ok(canon) = existing.canonicalize() {
        if !canon.starts_with(&croot) {
            return Err(format!("caminho escapa da skill: {rel:?}"));
        }
    }
    Ok(candidate)
}

// ------------------------------------------------------------------------------------------------
// Scaffold — cria o piso mínimo válido de uma skill.
// ------------------------------------------------------------------------------------------------

/// Cria `~/.claude/skills/schematize-<slug>/` com o piso mínimo VÁLIDO da casa e retorna o caminho.
/// Recusa se a pasta já existir (sem `--force`; use `scaffold_force` para sobrescrever).
/// `name` = nome humano (título/descrições); `description` = descrição densa do frontmatter.
pub fn scaffold(slug: &str, name: &str, description: &str) -> Result<PathBuf, String> {
    scaffold_in(&skills_dir(), slug, name, description, false)
}

/// Como `scaffold`, mas sobrescreve uma skill já existente (equivale ao `--force` da CLI).
pub fn scaffold_force(slug: &str, name: &str, description: &str) -> Result<PathBuf, String> {
    scaffold_in(&skills_dir(), slug, name, description, true)
}

/// Núcleo testável do scaffold: recebe a RAIZ de skills explicitamente (testes usam um tempdir),
/// valida o slug, cria a estrutura e grava os arquivos. `force` permite sobrescrever.
fn scaffold_in(
    root: &Path,
    slug: &str,
    name: &str,
    description: &str,
    force: bool,
) -> Result<PathBuf, String> {
    validate_slug(slug)?;
    let name = if name.trim().is_empty() { slug } else { name.trim() };
    let description = description.trim();

    let dest = root.join(format!("schematize-{slug}"));
    if dest.exists() {
        if !force {
            return Err(format!(
                "skill já existe: {} (use --force para sobrescrever)",
                dest.display()
            ));
        }
        fs::remove_dir_all(&dest).map_err(|e| format!("não consegui limpar {}: {e}", dest.display()))?;
    }

    // Estrutura de diretórios.
    fs::create_dir_all(dest.join("references")).map_err(|e| e.to_string())?;
    fs::create_dir_all(dest.join("assets").join("commands")).map_err(|e| e.to_string())?;

    // Arquivos do piso mínimo.
    write_new(&dest, "SKILL.md", &tpl_skill_md(slug, name, description))?;
    write_new(&dest, "skill.toml", &tpl_skill_toml(slug, name, description))?;
    write_new(&dest, "VERSION", &format!("{INITIAL_VERSION}\n"))?;
    write_new(&dest, "references/visao-geral.md", &tpl_reference(slug, name))?;
    write_new(
        &dest,
        &format!("assets/commands/{slug}-help.md"),
        &tpl_command_help(slug, name),
    )?;
    write_new(&dest, "assets/CLAUDE.md", &tpl_claude_md(slug, name, description))?;

    Ok(dest)
}

/// Grava um arquivo dentro da raiz recém-criada (criando pastas-pai). Uso interno do scaffold —
/// o caminho vem de constantes nossas, não de input do usuário, mas ainda passa por `join` seguro.
fn write_new(dest: &Path, rel: &str, content: &str) -> Result<(), String> {
    let p = dest.join(rel);
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&p, content).map_err(|e| format!("falha ao gravar {}: {e}", p.display()))
}

// ------------------------------------------------------------------------------------------------
// List / read / write dos arquivos editáveis.
// ------------------------------------------------------------------------------------------------

/// Lista os arquivos EDITÁVEIS de uma skill instalada, com caminhos RELATIVOS à raiz da skill:
/// `SKILL.md`, `skill.toml`, `references/*.md`, `assets/commands/*.md`. Ordem determinística.
pub fn list_files(slug: &str) -> Result<Vec<String>, String> {
    let root = skill_root(slug)?;
    list_files_in(&root)
}

/// Núcleo testável de `list_files`: recebe a raiz da skill diretamente.
fn list_files_in(root: &Path) -> Result<Vec<String>, String> {
    if !root.is_dir() {
        return Err(format!("skill não instalada: {}", root.display()));
    }
    let mut out: Vec<String> = Vec::new();

    // Arquivos-raiz sempre editáveis, se presentes.
    for f in ["SKILL.md", "skill.toml"] {
        if root.join(f).is_file() {
            out.push(f.to_string());
        }
    }
    // Markdown de references/ e assets/commands/.
    collect_md(root, "references", &mut out);
    collect_md(root, &format!("assets{}commands", std::path::MAIN_SEPARATOR), &mut out);

    out.sort();
    Ok(out)
}

/// Junta os `*.md` de um subdiretório (relativo à raiz) na lista, com caminho relativo `/`-separado.
fn collect_md(root: &Path, subdir: &str, out: &mut Vec<String>) {
    let dir = root.join(subdir);
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                    // Sempre com `/` — caminho relativo canônico da skill (multiplataforma no display).
                    out.push(format!("{}/{}", subdir.replace(std::path::MAIN_SEPARATOR, "/"), fname));
                }
            }
        }
    }
}

/// Lê UM arquivo dentro da skill (caminho relativo à raiz, validado contra traversal).
pub fn read_file(slug: &str, rel: &str) -> Result<String, String> {
    let root = skill_root(slug)?;
    let p = resolve_within(&root, rel)?;
    fs::read_to_string(&p).map_err(|e| format!("falha ao ler {}: {e}", p.display()))
}

/// Grava UM arquivo dentro da skill (caminho relativo à raiz, validado contra traversal).
/// Cria os diretórios-pai se preciso. Recusa qualquer caminho que escape da raiz.
pub fn write_file(slug: &str, rel: &str, content: &str) -> Result<(), String> {
    let root = skill_root(slug)?;
    let p = resolve_within(&root, rel)?;
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&p, content).map_err(|e| format!("falha ao gravar {}: {e}", p.display()))
}

// ------------------------------------------------------------------------------------------------
// Templates do piso mínimo (conteúdo válido e didático — exemplo pra editar).
// ------------------------------------------------------------------------------------------------

/// `SKILL.md`: frontmatter (`name: schematize-<slug>` + `metadata.version` + `description:` densa)
/// e corpo com seções. O `name` humano vira o título; a `description` é a que o Claude Code indexa.
fn tpl_skill_md(slug: &str, name: &str, description: &str) -> String {
    let desc = if description.is_empty() {
        format!("Skill {name} da casa — descreva aqui, de forma DENSA, o que ela cobre e QUANDO usar (gatilhos), pra o Claude Code disparar sozinho no contexto certo.")
    } else {
        description.to_string()
    };
    format!(
        r#"---
name: schematize-{slug}
metadata:
  version: {INITIAL_VERSION}
description: {desc}
---

# {name} (schematize-{slug})

Skill da casa recém-criada. Substitua este corpo pelo conteúdo normativo real: o que a skill
rege, os **pisos inegociáveis** (o que ela VETA), e como se usa. A `description` do frontmatter
acima é o que dispara a skill — mantenha-a densa e cheia de gatilhos.

## Comandos (Claude Code)

Digite `/{slug}-help` pra ver todos. Comece por este e vá adicionando em `assets/commands/`.

| Comando | O que faz |
|---|---|
| `/{slug}-help` | lista os comandos do schematize-{slug} |

Os comandos ficam em `assets/commands/` e são instalados em `.claude/commands/`.

## Como usar esta skill

1. Edite este `SKILL.md` com a disciplina real.
2. Detalhe cada tema em `references/*.md` — não trabalhe de memória.
3. Adicione comandos em `assets/commands/<slug>-*.md`.

Mapa de references:

| Tarefa | Reference |
|---|---|
| Visão geral e ponto de partida | `references/visao-geral.md` |

## Pisos inegociáveis (vetam o atalho)

1. Defina aqui os limites que esta skill NUNCA cruza. (Placeholder — substitua.)
"#
    )
}

/// `skill.toml`: metadados do catálogo/site (slug, name, tag, status, order, versão, href, descrições).
fn tpl_skill_toml(slug: &str, name: &str, description: &str) -> String {
    let d = if description.is_empty() {
        format!("Skill {name} da casa (rascunho).")
    } else {
        description.to_string()
    };
    // Escapa aspas pra não quebrar o TOML.
    let d = d.replace('\\', "\\\\").replace('"', "\\\"");
    let name_esc = name.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"slug = "{slug}"
name = "schematize-{slug}"
tag = "Skill · {name_esc}"
status = "draft"
order = 99
current_version = "{INITIAL_VERSION}"
href = "/{slug}"

[descriptions]
pt = "{d}"
en = "{d}"
"#
    )
}

/// `references/visao-geral.md`: exemplo de reference pra a pessoa autora seguir o formato.
fn tpl_reference(slug: &str, name: &str) -> String {
    format!(
        r#"# {name} — visão geral (schematize-{slug})

> Reference de exemplo. Cada reference cobre UM tema em profundidade; o `SKILL.md` só aponta
> pra cá. Substitua este conteúdo pelo corpo normativo real da skill.

## O que esta skill rege

Descreva o escopo: o que entra, o que fica de fora (e pra qual outra skill delega).

## Pisos (o que ela VETA)

- Piso 1 — (substitua).
- Piso 2 — (substitua).

## Passo a passo

1. (substitua)
2. (substitua)
"#
    )
}

/// `assets/commands/<slug>-help.md`: o comando `/help` da skill (frontmatter `description:` + corpo).
fn tpl_command_help(slug: &str, name: &str) -> String {
    format!(
        r#"---
description: schematize-{slug} — lista todos os comandos disponíveis e o que cada um faz
---

Liste os comandos do **schematize-{slug}** instalados (`/{slug}-*`), com 1 linha cada:

- `/{slug}-help` — esta lista.

Depois da lista, lembre a **regra de ouro** da skill {name}. Detalhe normativo em `references/`
da skill `schematize-{slug}`.
"#
    )
}

/// `assets/CLAUDE.md`: bloco sempre-on que a pessoa autora copia pra raiz do repo alvo.
fn tpl_claude_md(slug: &str, name: &str, description: &str) -> String {
    let d = if description.is_empty() {
        format!("A skill schematize-{slug} rege esta disciplina.")
    } else {
        description.to_string()
    };
    format!(
        r#"# CLAUDE.md — {name} (sempre on)

> Copie para a **raiz do repositório**. Fica pinado no contexto de toda tarefa e garante o piso
> mesmo quando a skill `schematize-{slug}` não dispara sozinha.

## Regra mestre

{d}

Em conflito entre um atalho e este piso, **o piso vence**. Consulte os references da skill
`schematize-{slug}` antes de agir — não trabalhe de memória.

## Pisos inegociáveis (VETADO — sem exceção)

1. (substitua pelos pisos reais da skill.)
"#
    )
}

// ------------------------------------------------------------------------------------------------
// Testes: tempdir + raiz isolada (sem tocar em $HOME, via os núcleos `*_in`).
// ------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Tempdir único por pid + contador atômico (testes em paralelo sem colisão).
    fn tmp() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let id = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("schematize_skilledit_{}_{id}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn slug_allow_list_deny_by_default() {
        assert!(validate_slug("demo").is_ok());
        assert!(validate_slug("demo-x").is_ok());
        assert!(validate_slug("a1b2").is_ok());
        // Recusados.
        assert!(validate_slug("").is_err());
        assert!(validate_slug("../etc").is_err());
        assert!(validate_slug("a/b").is_err());
        assert!(validate_slug("Demo").is_err()); // maiúscula
        assert!(validate_slug("-flag").is_err());
        assert!(validate_slug("demo-").is_err());
        assert!(validate_slug("foo..bar").is_err());
        assert!(validate_slug("a b").is_err());
    }

    #[test]
    fn scaffold_cria_estrutura_do_piso_minimo() {
        let root = tmp();
        let dest = scaffold_in(&root, "demo-x", "Demo", "teste denso", false).unwrap();
        assert!(dest.join("SKILL.md").is_file());
        assert!(dest.join("skill.toml").is_file());
        assert!(dest.join("VERSION").is_file());
        assert!(dest.join("references/visao-geral.md").is_file());
        assert!(dest.join("assets/commands/demo-x-help.md").is_file());
        assert!(dest.join("assets/CLAUDE.md").is_file());

        // Conteúdo válido: frontmatter com o name canônico e a versão inicial.
        let skill = fs::read_to_string(dest.join("SKILL.md")).unwrap();
        assert!(skill.contains("name: schematize-demo-x"));
        assert!(skill.contains(&format!("version: {INITIAL_VERSION}")));
        assert!(skill.contains("teste denso"));
        assert_eq!(fs::read_to_string(dest.join("VERSION")).unwrap().trim(), INITIAL_VERSION);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scaffold_recusa_se_ja_existe_sem_force() {
        let root = tmp();
        scaffold_in(&root, "dup", "Dup", "d", false).unwrap();
        // Segunda vez sem force = erro.
        assert!(scaffold_in(&root, "dup", "Dup", "d", false).is_err());
        // Com force = sobrescreve.
        assert!(scaffold_in(&root, "dup", "Dup", "d", true).is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_files_acha_os_editaveis() {
        let root = tmp();
        let dest = scaffold_in(&root, "lst", "Lst", "d", false).unwrap();
        let files = list_files_in(&dest).unwrap();
        assert!(files.contains(&"SKILL.md".to_string()));
        assert!(files.contains(&"skill.toml".to_string()));
        assert!(files.contains(&"references/visao-geral.md".to_string()));
        assert!(files.contains(&"assets/commands/lst-help.md".to_string()));
        // Determinístico (ordenado).
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn write_then_read_roundtrip() {
        let root = tmp();
        let dest = scaffold_in(&root, "rw", "Rw", "d", false).unwrap();
        // Grava um arquivo novo em subpasta (cria dirs-pai) e lê de volta.
        let p = resolve_within(&dest, "references/novo.md").unwrap();
        fs::write(&p, "conteúdo X").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "conteúdo X");
        // Sobrescreve SKILL.md via resolve.
        let sk = resolve_within(&dest, "SKILL.md").unwrap();
        fs::write(&sk, "novo corpo").unwrap();
        assert_eq!(fs::read_to_string(&sk).unwrap(), "novo corpo");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn path_traversal_e_recusado() {
        let root = tmp();
        let dest = scaffold_in(&root, "safe", "Safe", "d", false).unwrap();
        // `..` sobe pra fora da skill.
        assert!(resolve_within(&dest, "../etc/passwd").is_err());
        assert!(resolve_within(&dest, "../../etc/passwd").is_err());
        assert!(resolve_within(&dest, "references/../../escape").is_err());
        // Caminho absoluto.
        assert!(resolve_within(&dest, "/etc/passwd").is_err());
        assert!(resolve_within(&dest, "/abs").is_err());
        // Vazio.
        assert!(resolve_within(&dest, "").is_err());
        assert!(resolve_within(&dest, "   ").is_err());
        // Caminho válido interno passa.
        assert!(resolve_within(&dest, "references/ok.md").is_ok());
        fs::remove_dir_all(&root).ok();
    }
}
