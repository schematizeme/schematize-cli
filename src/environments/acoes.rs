//! As AÇÕES de verdade: instalar/remover uma linguagem ou uma ferramenta.

use super::*;

/// `schematize env install <slug> [--method <m>] [--dry-run] [--yes]`.
/// Aceita linguagem (go/rust/...) OU ferramenta (claude/code/codex). Pra ferramenta
/// o `--method` é ignorado (não há seletor — cada uma tem 1 caminho canônico).
pub fn install(lang: &str, method: Option<String>, dry_run: bool, yes: bool) -> Result<(), String> {
    if let Some(tool) = defs::find_tool(lang) {
        return install_tool(tool, method, dry_run, yes);
    }
    let env = defs::find(lang).ok_or_else(|| tf("env.unknown_lang", &[("lang", lang)]))?;
    let m = Machine::probe();

    // Sem --method: lista os disponíveis e PEDE um (não escolhe sozinho).
    let method = match method {
        Some(s) => Method::parse(&s).ok_or_else(|| tf("env.method_unknown", &[("method", &s)]))?,
        None => {
            let avail: Vec<&str> = m.available().iter().map(|x| x.slug()).collect();
            return Err(tf("env.method_required", &[("methods", &avail.join(", "))]));
        }
    };
    // Método inválido/indisponível → erro claro.
    if let Err(reason) = m.method_reason(method) {
        return Err(tf(
            "env.method_unavailable",
            &[("method", method.slug()), ("reason", &reason)],
        ));
    }

    // Idempotência: se já instalado por esse método (ou runtime já presente p/ distro/official).
    if already_installed(env, method, &m) {
        println!("{}", tf("env.already", &[("lang", env.display), ("method", method.slug())]));
        return Ok(());
    }

    let recipe = defs::install_recipe(env, method, m.family, m.mise);
    let steps = match recipe {
        Recipe::Steps(s) => s,
        Recipe::Todo(note) => {
            println!(
                "{}",
                tf(
                    "env.todo",
                    &[("lang", env.display), ("method", method.slug()), ("note", &note)]
                )
            );
            return Ok(());
        }
        Recipe::Na(note) => return Err(tf("env.na", &[("note", &note)])),
    };

    print_plan(env, method, &steps);
    for c in defs::tool_caveats(env.lang, method) {
        println!("    ! {} {}", t("env.caveat"), c);
    }
    if method == Method::Mise && !m.mise {
        println!("    ! {}", t("env.mise_bootstrap"));
    }

    // Consentimento (deny-by-default): dry-run não executa; senão --yes ou confirmação.
    let consent = if dry_run { false } else { yes || confirm() };
    match run_steps(&steps, dry_run, consent, exec_step)? {
        PlanAction::DryRun => {
            println!("{}", t("env.dry_run"));
            return Ok(());
        }
        PlanAction::Aborted => {
            println!("{}", t("env.aborted"));
            return Ok(());
        }
        PlanAction::Executed => {}
    }

    println!("{}", tf("env.done", &[("lang", env.display), ("method", method.slug())]));
    if method == Method::Docker {
        if let Some(img) = defs::docker_image(env.lang) {
            print_docker_usage(env, img);
        }
    }
    Ok(())
}

/// `schematize env remove <slug> [--method <m>] [--dry-run]`.
/// Aceita linguagem OU ferramenta; pra ferramenta o `--method` é ignorado.
pub fn remove(lang: &str, method: Option<String>, dry_run: bool) -> Result<(), String> {
    if let Some(tool) = defs::find_tool(lang) {
        return remove_tool(tool, method, dry_run);
    }
    let env = defs::find(lang).ok_or_else(|| tf("env.unknown_lang", &[("lang", lang)]))?;
    let m = Machine::probe();

    // Sem --method: detecta como foi instalado (docker/mise); se ambíguo, pede.
    let method = match method {
        Some(s) => Method::parse(&s).ok_or_else(|| tf("env.method_unknown", &[("method", &s)]))?,
        None => match installed_method(env, &m) {
            Some(mm) => mm,
            None => return Err(tf("env.remove_no_method", &[("lang", env.display)])),
        },
    };

    let recipe = defs::remove_recipe(env, method, m.family);
    let steps = match recipe {
        Recipe::Steps(s) => s,
        Recipe::Todo(note) => {
            println!(
                "{}",
                tf(
                    "env.todo",
                    &[("lang", env.display), ("method", method.slug()), ("note", &note)]
                )
            );
            return Ok(());
        }
        Recipe::Na(note) => return Err(tf("env.na", &[("note", &note)])),
    };

    println!("{}", tf("env.removing", &[("lang", env.display), ("method", method.slug())]));
    print_plan(env, method, &steps);

    // Remoção também respeita dry-run e consentimento.
    let consent = if dry_run { false } else { confirm() };
    match run_steps(&steps, dry_run, consent, exec_step)? {
        PlanAction::DryRun => println!("{}", t("env.dry_run")),
        PlanAction::Aborted => println!("{}", t("env.aborted")),
        PlanAction::Executed => {
            println!("{}", tf("env.done", &[("lang", env.display), ("method", method.slug())]))
        }
    }
    Ok(())
}

/// `schematize env install <tool>` — caminho canônico da ferramenta (por família no VS Code).
pub(crate) fn install_tool(
    tool: &Tool,
    method: Option<String>,
    dry_run: bool,
    yes: bool,
) -> Result<(), String> {
    warn_method_ignored(tool, &method);
    let m = Machine::probe();

    // Idempotência: se o binário já está no PATH, nada a fazer.
    if detect::has_bin(tool.bin) {
        println!("{}", tf("env.tool_already", &[("tool", tool.display)]));
        return Ok(());
    }

    let steps = match defs::tool_install_recipe(tool, m.family) {
        Recipe::Steps(s) => s,
        Recipe::Todo(note) => {
            println!("{}", tf("env.tool_todo", &[("tool", tool.display), ("note", &note)]));
            return Ok(());
        }
        Recipe::Na(note) => return Err(tf("env.na", &[("note", &note)])),
    };

    print_tool_plan(tool, &steps);

    // Consentimento (deny-by-default): dry-run não executa; senão --yes ou confirmação.
    let consent = if dry_run { false } else { yes || confirm() };
    match run_steps(&steps, dry_run, consent, exec_step)? {
        PlanAction::DryRun => println!("{}", t("env.dry_run")),
        PlanAction::Aborted => println!("{}", t("env.aborted")),
        PlanAction::Executed => {
            println!("{}", tf("env.tool_done", &[("tool", tool.display)]));
            // Prontidão pós-instalação: garante que o bin fique utilizável (PATH pronto).
            ensure_tool_ready(tool);
        }
    }
    Ok(())
}

/// `schematize env remove <tool>` — desfaz o caminho canônico (por família no VS Code).
pub(crate) fn remove_tool(
    tool: &Tool,
    method: Option<String>,
    dry_run: bool,
) -> Result<(), String> {
    warn_method_ignored(tool, &method);
    let m = Machine::probe();

    let steps = match defs::tool_remove_recipe(tool, m.family) {
        Recipe::Steps(s) => s,
        Recipe::Todo(note) => {
            println!("{}", tf("env.tool_todo", &[("tool", tool.display), ("note", &note)]));
            return Ok(());
        }
        Recipe::Na(note) => return Err(tf("env.na", &[("note", &note)])),
    };

    println!("{}", tf("env.tool_removing", &[("tool", tool.display)]));
    print_tool_plan(tool, &steps);

    let consent = if dry_run { false } else { confirm() };
    match run_steps(&steps, dry_run, consent, exec_step)? {
        PlanAction::DryRun => println!("{}", t("env.dry_run")),
        PlanAction::Aborted => println!("{}", t("env.aborted")),
        PlanAction::Executed => println!("{}", tf("env.tool_done", &[("tool", tool.display)])),
    }
    Ok(())
}

/// Prontidão pós-instalação de uma ferramenta — deixa o binário USÁVEL de verdade.
/// (i) reconfere o bin no PATH (`command -v`); (ii) se não estiver mas existir em
/// ~/.local/bin, garante ~/.local/bin no PATH (~/.bashrc E ~/.profile, idempotente)
/// e orienta reabrir o terminal ou `source`; (iii) se não apareceu em lugar nenhum,
/// avisa que a instalação pode ter falhado. Best-effort: nunca propaga erro.
pub(crate) fn ensure_tool_ready(tool: &Tool) {
    // (i) reconfere no PATH — se já resolve, está pronto pra uso.
    if detect::has_bin(tool.bin) {
        println!("{}", tf("env.ready_ok", &[("bin", tool.bin)]));
        return;
    }
    // (ii) não está no PATH — está em ~/.local/bin? Aí é só faltar o dir no PATH.
    let local_bin = util::home().join(".local").join("bin").join(tool.bin);
    if needs_path_fix(false, local_bin.exists()) {
        let mut changed = false;
        let mut failed = false;
        for rc in [".bashrc", ".profile"] {
            match ensure_export_in_rc(&util::home().join(rc)) {
                Ok(true) => changed = true,
                Ok(false) => {}
                Err(e) => {
                    failed = true;
                    println!("{}", tf("env.path_fix_failed", &[("error", &e)]));
                }
            }
        }
        if changed {
            println!("{}", t("env.path_fixed"));
        } else if !failed {
            println!("{}", t("env.path_already"));
        }
        return;
    }
    // (iii) nem no PATH nem em ~/.local/bin — a instalação pode ter falhado.
    println!("{}", tf("env.ready_missing", &[("bin", tool.bin)]));
}

/// `schematize env switch <lang> --to <method>` — troca o método de instalação.
///
/// **O quê:** descobre por onde a linguagem está instalada, instala pelo método novo e só
/// então remove o antigo.
///
/// **Onde:** despachado por `cli::diversos`.
///
/// ## A ordem é a garantia: instala o novo ANTES de remover o velho
///
/// O contrário — remover e depois instalar — deixa a pessoa **sem a linguagem** se o segundo
/// passo falhar por rede, por espaço em disco ou por um pacote que a distro não tem. Nesta
/// ordem, o pior caso é ficar com os dois instalados, que é chato e reversível.
///
/// ## Origem desconhecida NÃO é trocável
///
/// Se a procedência é [`Procedencia::Desconhecida`], a troca **recusa**. Remover pelo método
/// errado — um `rustup self uninstall` num binário que o rustup nunca viu — falha na melhor
/// das hipóteses e estraga outra coisa na pior. Dizer "não sei de onde isto veio, resolva à
/// mão" é a resposta honesta.
pub fn switch(lang: &str, para: &str, dry_run: bool, yes: bool) -> Result<(), String> {
    use super::procedencia::{self, Procedencia};

    let env = defs::ENVS
        .iter()
        .find(|e| e.lang == lang)
        .ok_or_else(|| format!("não conheço a linguagem `{lang}`. Veja `schematize env list`"))?;
    // `parse` devolve `Option`: nome desconhecido vira erro que LISTA o que existe, em vez
    // de um "None" que não ensina nada.
    let novo = Method::parse(para).ok_or_else(|| {
        format!(
            "método `{para}` não existe. Os que existem: {}",
            Method::ALL.iter().map(|x| x.slug()).collect::<Vec<_>>().join(", ")
        )
    })?;
    let m = Machine::probe();
    if !m.available().contains(&novo) {
        return Err(format!(
            "o método `{}` não está disponível nesta máquina. Disponíveis: {}",
            novo.slug(),
            m.available().iter().map(|x| x.slug()).collect::<Vec<_>>().join(", ")
        ));
    }

    let atual = procedencia::de(env.lang, env.bin, m.mise, m.docker);
    if !atual.instalado() {
        return Err(format!(
            "`{lang}` não está instalado — não há o que trocar. Use `schematize env install {lang} --method {}`",
            novo.slug()
        ));
    }
    if let Procedencia::Desconhecida { caminho } = &atual {
        return Err(format!(
            "`{lang}` está em {} e eu não sei quem o instalou — pode ter sido asdf, nvm, um \
             tarball ou a mão de alguém.\n  Não vou trocar: remover pelo método errado falha, \
             ou pior, mexe no que não devia.\n  Remova você por onde instalou, e depois rode \
             `schematize env install {lang} --method {}`",
            caminho.display(),
            novo.slug()
        ));
    }
    let antigo = atual.metodo().expect("instalado e conhecido tem método");
    if antigo == novo {
        println!("`{lang}` já está instalado {} — nada a trocar.", atual.rotulo());
        return Ok(());
    }

    let antigo_slug = antigo.slug();
    println!("TROCAR `{lang}`: {} → via {}", atual.rotulo(), novo.slug());
    println!();
    println!("  1. instala por `{}`", novo.slug());
    println!("  2. só então remove o `{}`", antigo.slug());
    println!();
    println!("  Nesta ordem de propósito: se o passo 1 falhar, você continua com o {antigo_slug}");
    println!("  que já tinha. A ordem inversa deixaria a máquina sem a linguagem.");
    if dry_run {
        println!();
        println!("(--dry-run: nada foi executado)");
        return Ok(());
    }
    if !yes {
        // A lib NÃO pergunta: quem tem terminal é a CLI. Sem `--yes` (ou `--dry-run`), a
        // troca não acontece — falha fechada, e a mensagem diz o que falta.
        println!();
        println!("Nada foi feito. Repita com `--yes` para executar, ou `--dry-run` para ver");
        println!("o plano sem tocar em nada.");
        return Ok(());
    }

    println!("\n── 1/2: instalando por {} ──", novo.slug());
    install(lang, Some(novo.slug().to_string()), false, true)?;

    // Reconfere ANTES de remover: se o install saiu 0 mas não deixou nada utilizável,
    // remover o antigo agora tiraria a única cópia funcional.
    let depois = procedencia::de(env.lang, env.bin, m.mise, m.docker);
    if !depois.instalado() {
        return Err(format!(
            "a instalação por `{}` terminou sem deixar `{lang}` utilizável. NÃO removi o `{}` \
             — você segue com o que tinha.",
            novo.slug(),
            antigo.slug()
        ));
    }

    println!("\n── 2/2: removendo o {} ──", antigo.slug());
    if let Err(e) = remove(lang, Some(antigo.slug().to_string()), false) {
        // Falhar aqui deixa os dois instalados. É chato e é reversível — e muito melhor que
        // o inverso. Dizer exatamente isso evita que a pessoa ache que perdeu algo.
        println!("aviso: não consegui remover o {}: {e}", antigo.slug());
        println!("       `{lang}` está instalado pelos DOIS métodos agora. Nada se perdeu;");
        println!(
            "       remova o antigo quando quiser: schematize env remove {lang} --method {}",
            antigo.slug()
        );
        return Ok(());
    }
    println!("\n✓ `{lang}` agora está via {}.", novo.slug());
    Ok(())
}
