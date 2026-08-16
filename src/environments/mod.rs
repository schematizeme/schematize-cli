//! Engine de ENVIRONMENTS — instala na máquina do usuário o runtime da linguagem +
//! ferramentas comuns, por 4 métodos que o usuário escolhe (docker | mise | distro | official).
//! O quê: lógica COMPARTILHADA (o CLI usa via `schematize env`; a GUI consumirá depois).
//! Onde: `defs` traz os dados/planos puros, `detect` sonda a máquina, e aqui mora a
//! orquestração: tabela, consentimento, dry-run e execução.
//!
//! SEGURANÇA (piso): nunca executa nada sem MOSTRAR o comando exato + procedência e obter
//! CONSENTIMENTO (ou --yes). `--dry-run` imprime tudo e não executa. Idempotente.

pub mod defs;
pub mod detect;

use crate::i18n::{t, tf};
use crate::util;
use defs::{Env, Method, Recipe, Step};
use detect::Family;
use std::io::{self, BufRead, Write};

/// Snapshot do que a máquina oferece — calculado uma vez por comando.
struct Machine {
    family: Family,
    mise: bool,
    docker: bool,
}

impl Machine {
    /// Sonda a máquina (família da distro, mise/docker presentes).
    fn probe() -> Machine {
        Machine {
            family: detect::family(),
            mise: detect::has_mise(),
            docker: detect::has_docker(),
        }
    }

    /// Métodos DISPONÍVEIS nesta máquina, em ordem estável.
    /// mise/official sempre disponíveis (bootstrappáveis); docker só com docker; distro só com família.
    fn available(&self) -> Vec<Method> {
        Method::ALL
            .into_iter()
            .filter(|m| self.method_reason(*m).is_ok())
            .collect()
    }

    /// Ok se o método é utilizável aqui; Err(razão) caso contrário (deny-by-default).
    fn method_reason(&self, m: Method) -> Result<(), String> {
        match m {
            Method::Docker if !self.docker => Err("docker não encontrado no PATH.".into()),
            Method::Distro if self.family == Family::Unknown => {
                Err("família da distro não detectada (/etc/os-release).".into())
            }
            _ => Ok(()),
        }
    }
}

/// Como um environment está instalado nesta máquina (pra tabela e idempotência).
fn installed_method(env: &Env, m: &Machine) -> Option<Method> {
    if m.docker {
        if let Some(img) = defs::docker_image(env.lang) {
            if detect::docker_image_present(img) {
                return Some(Method::Docker);
            }
        }
    }
    if m.mise && detect::mise_has(defs::mise_tools(env.lang).last().copied().unwrap_or("")) {
        return Some(Method::Mise);
    }
    None
}

/// Texto de status pra a tabela: instalado por qual método, ou só "instalado", ou não.
fn status_text(env: &Env, m: &Machine) -> String {
    if let Some(method) = installed_method(env, m) {
        return tf("env.installed_via", &[("method", method.slug())]);
    }
    if detect::has_bin(env.bin) {
        return t("env.installed");
    }
    t("env.not_installed")
}

/// `schematize env list` — tabela: linguagem, métodos disponíveis aqui, e status.
pub fn list() {
    let m = Machine::probe();
    let avail: Vec<&str> = m.available().iter().map(|x| x.slug()).collect();
    println!("{}", t("env.header"));
    println!(
        "  {:<12} {:<32} {}",
        t("env.col_lang"),
        t("env.col_methods"),
        t("env.col_status")
    );
    for env in defs::ENVS {
        // métodos disponíveis são os mesmos pra a máquina; mostramos por linha por clareza.
        let methods = avail.join(", ");
        println!(
            "  {:<12} {:<32} {}",
            env.display,
            methods,
            status_text(env, &m)
        );
    }
}

/// Imprime o plano (cada passo: comando exato + procedência + selos sudo/pipe|sh).
fn print_plan(env: &Env, method: Method, steps: &[Step]) {
    println!(
        "{}",
        tf("env.plan_title", &[("lang", env.display), ("method", method.slug())])
    );
    for s in steps {
        if s.source == "nota" {
            // passo informativo (no-op) — mostra só a orientação, não como comando.
            let msg = s.cmd.trim_start_matches(": # ");
            println!("    · {msg}");
            continue;
        }
        let mut tags = String::new();
        if s.sudo {
            tags.push_str(&format!(" {}", t("env.tag_sudo")));
        }
        if s.pipe_sh {
            tags.push_str(&format!(" {}", t("env.tag_pipe")));
        }
        println!("    $ {}{}", s.cmd, tags);
        println!("        {} {}", t("env.source"), s.source);
    }
}

/// Pergunta interativa de consentimento (y/N). Falha fechada: erro/EOF = não.
fn confirm() -> bool {
    print!("{} ", t("env.confirm"));
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes" | "s" | "sim")
}

/// O que fazer com um plano depois de exibido — decisão PURA (testável sem I/O).
#[derive(Debug, PartialEq, Eq)]
pub enum PlanAction {
    /// dry-run: nada executado.
    DryRun,
    /// sem consentimento: abortado, nada executado.
    Aborted,
    /// executado (todos os passos rodaram).
    Executed,
}

/// Núcleo testável da execução: decide e roda os passos via o runner injetado.
/// Com `dry_run` NUNCA chama o runner; sem `consent` também não. Retorna o que ocorreu.
pub fn run_steps<R>(steps: &[Step], dry_run: bool, consent: bool, mut run: R) -> Result<PlanAction, String>
where
    R: FnMut(&Step) -> Result<(), String>,
{
    if dry_run {
        return Ok(PlanAction::DryRun);
    }
    if !consent {
        return Ok(PlanAction::Aborted);
    }
    for s in steps {
        run(s)?;
    }
    Ok(PlanAction::Executed)
}

/// Executa um passo de verdade (a menos que seja nota/no-op).
fn exec_step(s: &Step) -> Result<(), String> {
    if s.source == "nota" {
        return Ok(());
    }
    util::run_shell(&s.cmd)
}

/// `schematize env install <lang> --method <m> [--dry-run] [--yes]`.
pub fn install(lang: &str, method: Option<String>, dry_run: bool, yes: bool) -> Result<(), String> {
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
            println!("{}", tf("env.todo", &[("lang", env.display), ("method", method.slug()), ("note", &note)]));
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

/// Idempotência: environment já satisfeito por este método?
fn already_installed(env: &Env, method: Method, m: &Machine) -> bool {
    match method {
        Method::Docker => defs::docker_image(env.lang)
            .map(detect::docker_image_present)
            .unwrap_or(false),
        Method::Mise => {
            m.mise && detect::mise_has(defs::mise_tools(env.lang).last().copied().unwrap_or(""))
        }
        // distro/official: se o runtime já está no PATH, o objetivo do env está atendido.
        Method::Distro | Method::Official => detect::has_bin(env.bin),
    }
}

/// Imprime como USAR a imagem docker recém-baixada (docker run + snippet devcontainer).
fn print_docker_usage(env: &Env, img: &str) {
    println!("{}", t("env.docker_usage"));
    println!(
        "    $ docker run --rm -it -v \"$PWD\":/work -w /work {img} {}",
        env.bin
    );
    println!("    .devcontainer/devcontainer.json:");
    println!("      {{ \"image\": \"{img}\" }}");
}

/// `schematize env remove <lang> [--method <m>] [--dry-run]`.
pub fn remove(lang: &str, method: Option<String>, dry_run: bool) -> Result<(), String> {
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
            println!("{}", tf("env.todo", &[("lang", env.display), ("method", method.slug()), ("note", &note)]));
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
        PlanAction::Executed => println!("{}", tf("env.done", &[("lang", env.display), ("method", method.slug())])),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Toda combinação lang × método resolve sem panic e ou tem passos, ou traz um
    /// motivo (Todo/Na) NÃO-vazio — nada de combinação silenciosamente vazia.
    #[test]
    fn matriz_lang_metodo_completa() {
        for env in defs::ENVS {
            for method in Method::ALL {
                for fam in [Family::Debian, Family::Rpm, Family::Unknown] {
                    let r = defs::install_recipe(env, method, fam, true);
                    match r {
                        Recipe::Steps(s) => assert!(!s.is_empty(), "{} {:?}: passos vazios", env.lang, method),
                        Recipe::Todo(n) | Recipe::Na(n) => {
                            assert!(!n.trim().is_empty(), "{} {:?}: motivo vazio", env.lang, method)
                        }
                    }
                    // remoção idem
                    let _ = defs::remove_recipe(env, method, fam);
                }
            }
        }
    }

    /// Os 7 environments existem e cada um tem runtime + ao menos 1 ferramenta.
    #[test]
    fn tabela_sete_envs() {
        let langs = ["go", "rust", "elixir", "csharp", "zig", "ruby", "node"];
        assert_eq!(defs::ENVS.len(), 7);
        for l in langs {
            let e = defs::find(l).expect("env presente");
            assert!(!e.runtime.is_empty());
            assert!(!e.tools.is_empty());
            assert!(!e.bin.is_empty());
        }
    }

    /// Detecção de família por os-release falso (parse puro).
    #[test]
    fn familia_por_os_release_falso() {
        let ubuntu = "ID=ubuntu\nID_LIKE=debian\n";
        let mint = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
        let suse = "ID=\"opensuse-leap\"\nID_LIKE=\"suse opensuse\"\n";
        let fedora = "ID=fedora\n";
        let arch = "ID=arch\nID_LIKE=\n";
        assert_eq!(detect::family_from(ubuntu), Family::Debian);
        assert_eq!(detect::family_from(mint), Family::Debian);
        assert_eq!(detect::family_from(suse), Family::Rpm);
        assert_eq!(detect::family_from(fedora), Family::Rpm);
        assert_eq!(detect::family_from(arch), Family::Unknown);
        assert_eq!(detect::family_from(""), Family::Unknown);
    }

    /// dry-run NUNCA chama o runner (não executa nada).
    #[test]
    fn dry_run_nao_executa() {
        let steps = vec![Step { cmd: "echo x".into(), source: "t".into(), sudo: false, pipe_sh: false }];
        let mut calls = 0;
        let action = run_steps(&steps, true, true, |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(action, PlanAction::DryRun);
        assert_eq!(calls, 0, "dry-run não pode executar passos");
    }

    /// Sem consentimento também não executa.
    #[test]
    fn sem_consentimento_aborta() {
        let steps = vec![Step { cmd: "echo x".into(), source: "t".into(), sudo: false, pipe_sh: false }];
        let mut calls = 0;
        let action = run_steps(&steps, false, false, |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(action, PlanAction::Aborted);
        assert_eq!(calls, 0);
    }

    /// Com consentimento e sem dry-run, roda todos os passos.
    #[test]
    fn consentido_executa_todos() {
        let steps = vec![
            Step { cmd: "a".into(), source: "t".into(), sudo: false, pipe_sh: false },
            Step { cmd: "b".into(), source: "t".into(), sudo: false, pipe_sh: false },
        ];
        let mut calls = 0;
        let action = run_steps(&steps, false, true, |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(action, PlanAction::Executed);
        assert_eq!(calls, 2);
    }

    /// Parse de método é deny-by-default (desconhecido = None).
    #[test]
    fn parse_metodo_deny_default() {
        assert_eq!(Method::parse("docker"), Some(Method::Docker));
        assert_eq!(Method::parse("mise"), Some(Method::Mise));
        assert_eq!(Method::parse("distro"), Some(Method::Distro));
        assert_eq!(Method::parse("official"), Some(Method::Official));
        assert_eq!(Method::parse("apt"), None);
        assert_eq!(Method::parse(""), None);
    }
}
