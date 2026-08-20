//! Subcomandos de SKILLS: instalar, atualizar, remover, listar, forkar, comparar,
//! criar do zero e editar.

use schematize::i18n::{t, tf};
use schematize::{
    registry, skilledit,
    skills,
};
use schematize::{market};
use crate::cli::args::*;

pub(crate) fn resolve(cat: &[registry::Item], names: &[String], all: bool) -> Vec<registry::Item> {
    if all || names.is_empty() {
        cat.to_vec()
    } else {
        names.iter().filter_map(|n| registry::find(cat, n)).collect()
    }
}

/// `schematize skills <sub>` — dispatcher da gestão de skills (a feature).
pub(crate) fn skills_cmd(sub: SkillsCmd) -> Result<(), String> {
    match sub {
        SkillsCmd::Applied { mark } => crate::cli::skillsproj::applied_cmd(mark),
        SkillsCmd::Rerun { slug } => crate::cli::skillsproj::rerun_cmd(slug),
        SkillsCmd::Install { names, all, with_recommended } => {
            skills_install(&names, all, with_recommended)
        }
        SkillsCmd::Update { names, all } => skills_update(&names, all),
        SkillsCmd::List => skills_list(),
        SkillsCmd::Remove { name } => skills_remove(&name),
        SkillsCmd::New { slug, name, desc, force } => skills_new(&slug, name, desc, force),
        SkillsCmd::Edit { slug, list, file, set_from } => skills_edit(&slug, list, file, set_from),
        SkillsCmd::Fork { slug } => skills_fork(&slug),
        SkillsCmd::Compare { slug } => skills_compare(&slug),
    }
}

/// `schematize skills fork <slug>` — força o fork de uma skill oficial (guarda a base no stash).
pub(crate) fn skills_fork(slug: &str) -> Result<(), String> {
    if !skills::is_official(slug) {
        println!("skill {slug} não é oficial (do catálogo) — ela já edita livremente, sem fork.");
        return Ok(());
    }
    skills::fork(slug)?;
    println!("skill {slug} forkada: a pasta ativa é editável e a base oficial ficou guardada no stash.");
    println!("compare depois com: schematize skills compare {slug}");
    Ok(())
}

/// `schematize skills compare <slug>` — mostra o diff do fork ativo vs a nova oficial (latest).
pub(crate) fn skills_compare(slug: &str) -> Result<(), String> {
    let cmp = skills::compare_update(slug)?;
    println!("Comparando fork de {slug}: base v{} → nova oficial v{}", cmp.base_version, cmp.new_version);
    if cmp.files.is_empty() {
        println!("  (nenhum arquivo — nada a comparar)");
    }
    for f in &cmp.files {
        println!("  {:<10} {}", f.status, f.path);
    }
    if !cmp.diff_text.trim().is_empty() {
        println!("\n--- diff unificado (fork ativo → nova oficial) ---");
        print!("{}", cmp.diff_text);
    }
    Ok(())
}

/// `schematize skills new <slug>` — scaffolda o piso mínimo válido de uma skill nova.
pub(crate) fn skills_new(slug: &str, name: Option<String>, desc: Option<String>, force: bool) -> Result<(), String> {
    let name = name.unwrap_or_else(|| slug.to_string());
    let desc = desc.unwrap_or_default();
    let dest = if force {
        skilledit::scaffold_force(slug, &name, &desc)?
    } else {
        skilledit::scaffold(slug, &name, &desc)?
    };
    println!("{}", tf("skilledit.created", &[("path", &dest.display().to_string())]));
    Ok(())
}

/// `schematize skills edit <slug>` — lista os arquivos, imprime um, ou o grava de um arquivo fonte.
pub(crate) fn skills_edit(slug: &str, list: bool, file: Option<String>, set_from: Option<String>) -> Result<(), String> {
    // Com --file: escreve (se --set-from) ou imprime o conteúdo.
    if let Some(rel) = file {
        if let Some(src) = set_from {
            let content = std::fs::read_to_string(&src).map_err(|e| format!("falha ao ler {src}: {e}"))?;
            skilledit::write_file(slug, &rel, &content)?;
            println!("{}", tf("skilledit.wrote", &[("file", &rel)]));
            return Ok(());
        }
        let content = skilledit::read_file(slug, &rel)?;
        print!("{content}");
        return Ok(());
    }
    // Sem --file: lista (o default quando nada mais é passado). `--list` é o mesmo comportamento.
    let _ = list;
    let files = skilledit::list_files(slug)?;
    println!("{}", tf("skilledit.files_header", &[("slug", slug)]));
    for f in files {
        println!("  {f}");
    }
    Ok(())
}

/// Instala skills (ou todas com --all) e, opcionalmente, as recomendadas.
pub(crate) fn skills_install(names: &[String], all: bool, with_recommended: bool) -> Result<(), String> {
    let cat = registry::catalog();
    let selected = resolve(&cat, names, all);
    for it in &selected {
        match skills::install(it) {
            Ok(v) => println!("✓ {}", tf("skills.installed_ok", &[("name", &it.slug), ("v", &v)])),
            Err(e) => eprintln!("✗ {}: {e}", it.slug),
        }
    }
    // Recomendações (skill BASE complementar). Nunca instala de surpresa:
    // com --all já vem tudo; senão sugere, e só instala com --with-recommended.
    if !all {
        let mut suggested: Vec<String> = Vec::new();
        for it in &selected {
            for rec in &it.recommends {
                let already = registry::find(&cat, rec)
                    .map(|r| skills::installed_version(&r).is_some())
                    .unwrap_or(false);
                let in_batch = selected.iter().any(|s| &s.slug == rec);
                if !already && !in_batch && !suggested.contains(rec) {
                    suggested.push(rec.clone());
                }
            }
        }
        if !suggested.is_empty() {
            if with_recommended {
                for rec in &suggested {
                    if let Some(r) = registry::find(&cat, rec) {
                        match skills::install(&r) {
                            Ok(v) => println!("✓ {}", tf("skills.installed_ok", &[("name", &r.slug), ("v", &v)])),
                            Err(e) => eprintln!("✗ {}: {e}", r.slug),
                        }
                    }
                }
            } else {
                println!("{}", tf("skills.recommends_hint", &[("list", &suggested.join(", "))]));
            }
        }
    }
    Ok(())
}

/// Atualiza skills instaladas pro latest (todas se não passar nome/--all). Skills FORKADAS
/// não são sobrescritas — `skills::update` recusa e aponta o caminho comparar/mesclar.
pub(crate) fn skills_update(names: &[String], all: bool) -> Result<(), String> {
    let cat = registry::catalog();
    for it in resolve(&cat, names, all) {
        match skills::update(&it) {
            Ok(v) => println!("✓ {}", tf("skills.updated", &[("name", &it.slug), ("v", &v)])),
            Err(e) => eprintln!("✗ {}: {e}", it.slug),
        }
    }
    Ok(())
}

/// Lista skills: instaladas vs última disponível. Se houver rede, anexa a nota do marketplace
/// (uma única request pra todas as linhas) — offline não trava: o mapa vem vazio e a nota some.
pub(crate) fn skills_list() -> Result<(), String> {
    let st = skills::load_state();
    let ratings = market::market_ratings_all(); // vazio se offline; não bloqueia a listagem
    println!("{}", t("skills.header"));
    for it in &registry::catalog() {
        let line = skills::status_line(it, &st, true);
        let nota = market::format_rating(ratings.get(&it.slug).copied());
        if nota.is_empty() {
            println!("  {line}");
        } else {
            println!("  {line}  {nota}");
        }
    }
    Ok(())
}

/// Remove uma skill instalada.
pub(crate) fn skills_remove(name: &str) -> Result<(), String> {
    let cat = registry::catalog();
    match registry::find(&cat, name) {
        Some(it) => skills::remove(&it).map(|_| println!("{}", tf("skills.removed", &[("name", &it.slug)]))),
        None => Err(tf("skills.unknown", &[("name", name)])),
    }
}
