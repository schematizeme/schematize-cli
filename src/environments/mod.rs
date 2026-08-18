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
use defs::{Env, Recipe, Step, Tool};
use detect::Family;
use std::io::{self, BufRead, Write};

// Re-exporta `Method` no nível do módulo (traz pra escopo interno E expõe como
// `environments::Method` pra consumidores externos, ex.: a GUI).
pub use defs::Method;


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

// ---------------------------------------------------------------------------
// API de DADOS pública (aditiva): o status estruturado de cada environment nesta
// máquina — pra a GUI consumir SEM parsear texto. O `list()` também passa a
// consumir isto (fonte única; nada de duplicar a detecção). O egui não usa.
// ---------------------------------------------------------------------------

/// Status estruturado de UM environment nesta máquina.
/// Cobre linguagens E ferramentas: a GUI agrupa/distingue por `category`
/// ("language" | "tool"). Ferramentas não têm método (`methods_available` vazio,
/// `installed` sempre None) — seu status vem só de `runtime_present` (bin no PATH).
pub struct LangEnv {
    /// slug curto ("go", "rust", "claude", "code", ...).
    pub lang: &'static str,
    /// nome de exibição ("Go", "C# / .NET", "Claude Code", ...).
    pub display: &'static str,
    /// categoria pra a GUI agrupar: "language" (runtime) | "tool" (ferramenta de dev).
    pub category: &'static str,
    /// rótulo do caminho de instalação (linguagem: métodos disponíveis; ferramenta: fonte canônica).
    pub install_hint: String,
    /// métodos utilizáveis NESTA máquina (docker só com docker; distro só com família).
    /// Vazio pra ferramentas (não usam os 4 métodos).
    pub methods_available: Vec<Method>,
    /// método de instalação DETECTADO (docker/mise), quando rastreável; None caso contrário
    /// (e sempre None pra ferramentas).
    pub installed: Option<Method>,
    /// runtime/binário já presente no PATH (cobre distro/official e TODAS as ferramentas).
    pub runtime_present: bool,
}

impl LangEnv {
    /// A GUI/tabela consideram "instalado" se há método detectado OU o runtime no PATH.
    pub fn is_installed(&self) -> bool {
        self.installed.is_some() || self.runtime_present
    }
}

/// Status de TODOS os environments nesta máquina (sonda a máquina UMA vez).
/// Lista linguagens PRIMEIRO, ferramentas depois. Fonte única (o `list()` e a GUI
/// consomem isto). Reaproveita exatamente a detecção que a tabela usa.
pub fn status() -> Vec<LangEnv> {
    let m = Machine::probe();
    let available = m.available();
    let langs = defs::ENVS.iter().map(|env| LangEnv {
        lang: env.lang,
        display: env.display,
        category: "language",
        install_hint: available.iter().map(|x| x.slug()).collect::<Vec<_>>().join(", "),
        methods_available: available.clone(),
        installed: installed_method(env, &m),
        runtime_present: detect::has_bin(env.bin),
    });
    let tools = defs::TOOLS.iter().map(|tool| LangEnv {
        lang: tool.slug,
        display: tool.display,
        category: "tool",
        install_hint: tool.source_hint.to_string(),
        methods_available: Vec::new(),
        installed: None,
        runtime_present: detect::has_bin(tool.bin),
    });
    langs.chain(tools).collect()
}

/// Texto de status pra a tabela: instalado por qual método, ou só "instalado", ou não.
fn status_text(le: &LangEnv) -> String {
    if let Some(method) = le.installed {
        return tf("env.installed_via", &[("method", method.slug())]);
    }
    if le.runtime_present {
        return t("env.installed");
    }
    t("env.not_installed")
}

/// `schematize env list` — tabela: nome, caminho de instalação, e status.
/// Linguagens e ferramentas na mesma tabela, com um cabeçalho por seção.
pub fn list() {
    let envs = status();
    println!("{}", t("env.header"));
    println!(
        "  {:<14} {:<34} {}",
        t("env.col_lang"),
        t("env.col_methods"),
        t("env.col_status")
    );
    let mut printed_tools_header = false;
    for le in &envs {
        // Um cabeçalho de seção quando começam as ferramentas.
        if le.category == "tool" && !printed_tools_header {
            println!("{}", t("env.tools_header"));
            printed_tools_header = true;
        }
        println!("  {:<14} {:<34} {}", le.display, le.install_hint, status_text(le));
    }
}

/// Imprime o plano de uma LINGUAGEM (título + passos).
fn print_plan(env: &Env, method: Method, steps: &[Step]) {
    println!(
        "{}",
        tf("env.plan_title", &[("lang", env.display), ("method", method.slug())])
    );
    print_steps(steps);
}

/// Imprime o plano de uma FERRAMENTA (título próprio + passos).
fn print_tool_plan(tool: &Tool, steps: &[Step]) {
    println!("{}", tf("env.tool_plan_title", &[("tool", tool.display)]));
    print_steps(steps);
}

/// Imprime a lista de passos (comando exato + procedência + selos sudo/pipe|sh).
/// Compartilhado por linguagens e ferramentas — o guardrail é o mesmo pra ambos.
fn print_steps(steps: &[Step]) {
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

// ---------------------------------------------------------------------------
// FERRAMENTAS: instalação/remoção. Reusa o MESMO guardrail (mostra o comando,
// pede consentimento, respeita dry-run) e o mesmo runner das linguagens. Sem
// seletor de método: se vier `--method`, avisa e ignora.
// ---------------------------------------------------------------------------

/// Avisa (uma vez) que o `--method` é ignorado pra ferramentas.
fn warn_method_ignored(tool: &Tool, method: &Option<String>) {
    if let Some(mm) = method {
        println!("{}", tf("env.tool_method_ignored", &[("tool", tool.display), ("method", mm)]));
    }
}

/// `schematize env install <tool>` — caminho canônico da ferramenta (por família no VS Code).
fn install_tool(tool: &Tool, method: Option<String>, dry_run: bool, yes: bool) -> Result<(), String> {
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
fn remove_tool(tool: &Tool, method: Option<String>, dry_run: bool) -> Result<(), String> {
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

// ---------------------------------------------------------------------------
// PRONTIDÃO PÓS-INSTALAÇÃO. Muitos instaladores oficiais (Claude Code, e qualquer
// coisa via curl|sh) jogam o binário em ~/.local/bin — que NEM sempre está no PATH
// do usuário. Sem isso, o dev instala e o comando "não abre" nem reabrindo o
// terminal. Aqui a instalação vira SELF-VERIFYING: reconfere o bin no PATH e, se
// faltar, garante ~/.local/bin no PATH (~/.bashrc + ~/.profile, idempotente) e
// orienta como recarregar. Best-effort: NUNCA quebra a instalação já concluída.
// ---------------------------------------------------------------------------

/// A linha de export que garante ~/.local/bin no PATH do usuário.
const LOCAL_BIN_EXPORT: &str = "export PATH=\"$HOME/.local/bin:$PATH\"";

/// Decisão PURA: precisa consertar o PATH? Só quando o binário NÃO está no PATH
/// mas EXISTE em ~/.local/bin — nesse caso, pôr ~/.local/bin no PATH resolve. Se
/// nem no PATH nem no ~/.local/bin, o problema é outro (instalação falhou) e não
/// há PATH a consertar. Testável sem tocar o disco.
pub fn needs_path_fix(bin_in_path: bool, bin_in_local_bin: bool) -> bool {
    !bin_in_path && bin_in_local_bin
}

/// Decisão PURA e idempotente: o conteúdo de um rc já garante ~/.local/bin no PATH?
/// Considera presente qualquer linha NÃO-comentada que exporte um PATH mencionando
/// `.local/bin`. Assim não duplicamos a linha em quem já a tem. Testável com string.
pub fn rc_already_has_local_bin(content: &str) -> bool {
    content.lines().any(|l| {
        let l = l.trim();
        !l.starts_with('#') && l.contains(".local/bin") && l.contains("PATH")
    })
}

/// Garante (idempotente, best-effort) a linha de export num arquivo rc. Cria o
/// arquivo se não existir (ex.: ~/.bashrc ausente). Retorna Ok(true) se ADICIONOU
/// a linha, Ok(false) se já estava lá, Err se não deu pra escrever.
fn ensure_export_in_rc(path: &std::path::Path) -> Result<bool, String> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if rc_already_has_local_bin(&existing) {
        return Ok(false);
    }
    let mut new = existing;
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str("\n# schematize: garante ~/.local/bin no PATH\n");
    new.push_str(LOCAL_BIN_EXPORT);
    new.push('\n');
    std::fs::write(path, &new).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

/// Prontidão pós-instalação de uma ferramenta — deixa o binário USÁVEL de verdade.
/// (i) reconfere o bin no PATH (`command -v`); (ii) se não estiver mas existir em
/// ~/.local/bin, garante ~/.local/bin no PATH (~/.bashrc E ~/.profile, idempotente)
/// e orienta reabrir o terminal ou `source`; (iii) se não apareceu em lugar nenhum,
/// avisa que a instalação pode ter falhado. Best-effort: nunca propaga erro.
fn ensure_tool_ready(tool: &Tool) {
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

    /// As 3 ferramentas existem, têm bin/slug/hint e aparecem no status() (categoria "tool").
    #[test]
    fn tabela_tres_ferramentas() {
        let slugs = ["claude", "code", "codex"];
        assert_eq!(defs::TOOLS.len(), 3);
        for s in slugs {
            let t = defs::find_tool(s).expect("ferramenta presente");
            assert!(!t.display.is_empty() && !t.bin.is_empty() && !t.source_hint.is_empty());
        }
        // status() lista linguagens E ferramentas; as 3 ferramentas estão lá como "tool".
        let all = status();
        for s in slugs {
            let le = all.iter().find(|e| e.lang == s).expect("ferramenta no status");
            assert_eq!(le.category, "tool");
            assert!(le.methods_available.is_empty(), "ferramenta não usa método");
            assert!(le.installed.is_none());
        }
        // E as linguagens seguem como "language".
        assert_eq!(all.iter().find(|e| e.lang == "go").unwrap().category, "language");
    }

    /// VS Code na família Debian gera o comando do .deb oficial (com sudo).
    #[test]
    fn vscode_debian_gera_deb() {
        let tool = defs::find_tool("code").unwrap();
        let steps = match defs::tool_install_recipe(tool, Family::Debian) {
            Recipe::Steps(s) => s,
            _ => panic!("esperava Steps pra VS Code em Debian"),
        };
        assert_eq!(steps.len(), 1);
        assert!(steps[0].cmd.contains("os=linux-deb-x64"), "baixa o .deb oficial");
        assert!(steps[0].cmd.contains("apt-get install -y /tmp/schematize-vscode.deb"));
        assert!(steps[0].sudo, "instalar o .deb exige sudo");
    }

    /// VS Code no Rpm usa o repo oficial da Microsoft (3 passos: chave, repo, install).
    #[test]
    fn vscode_rpm_usa_repo_microsoft() {
        let tool = defs::find_tool("code").unwrap();
        let steps = match defs::tool_install_recipe(tool, Family::Rpm) {
            Recipe::Steps(s) => s,
            _ => panic!("esperava Steps pra VS Code em Rpm"),
        };
        assert_eq!(steps.len(), 3);
        assert!(steps[0].cmd.contains("rpm --import https://packages.microsoft.com/keys/microsoft.asc"));
        assert!(steps[2].cmd.contains("dnf install -y code") && steps[2].cmd.contains("zypper"));
    }

    /// VS Code em família desconhecida é N/A (não chuta gerenciador de pacotes).
    #[test]
    fn vscode_familia_desconhecida_na() {
        let tool = defs::find_tool("code").unwrap();
        assert!(matches!(defs::tool_install_recipe(tool, Family::Unknown), Recipe::Na(_)));
    }

    /// Claude Code e Codex têm caminho canônico único (independe de família).
    #[test]
    fn claude_e_codex_caminho_canonico() {
        let claude = defs::find_tool("claude").unwrap();
        for fam in [Family::Debian, Family::Rpm, Family::Unknown] {
            match defs::tool_install_recipe(claude, fam) {
                Recipe::Steps(s) => {
                    assert_eq!(s.len(), 1);
                    assert!(s[0].cmd.contains("claude.ai/install.sh"));
                    assert!(s[0].pipe_sh, "curl|bash = código remoto (selo pipe)");
                }
                _ => panic!("claude sempre tem Steps"),
            }
        }
        let codex = defs::find_tool("codex").unwrap();
        match defs::tool_install_recipe(codex, Family::Debian) {
            Recipe::Steps(s) => {
                // note-step da dependência de node + o npm install.
                assert!(s.iter().any(|st| st.cmd.contains("npm install -g @openai/codex")));
                assert!(s.iter().any(|st| st.source == "nota"), "cita a dependência de Node.js");
            }
            _ => panic!("codex sempre tem Steps"),
        }
    }

    /// Remoção das ferramentas é coerente (claude remove o bin; code por família; codex npm).
    #[test]
    fn remocao_ferramentas() {
        let claude = defs::find_tool("claude").unwrap();
        match defs::tool_remove_recipe(claude, Family::Unknown) {
            Recipe::Steps(s) => assert!(s[0].cmd.contains(".local/bin/claude")),
            _ => panic!("claude remove tem Steps"),
        }
        let code = defs::find_tool("code").unwrap();
        match defs::tool_remove_recipe(code, Family::Debian) {
            Recipe::Steps(s) => assert!(s[0].cmd.contains("apt-get remove -y code")),
            _ => panic!("code debian remove tem Steps"),
        }
        assert!(matches!(defs::tool_remove_recipe(code, Family::Unknown), Recipe::Na(_)));
        let codex = defs::find_tool("codex").unwrap();
        match defs::tool_remove_recipe(codex, Family::Rpm) {
            Recipe::Steps(s) => assert!(s[0].cmd.contains("npm uninstall -g @openai/codex")),
            _ => panic!("codex remove tem Steps"),
        }
    }

    /// O recipe do Codex agora roda com SUDO (conserta o EACCES no npm global do sistema):
    /// o comando traz `sudo` literal E o selo de sudo está marcado (guardrail exibe [sudo]).
    #[test]
    fn codex_instala_com_sudo() {
        let codex = defs::find_tool("codex").unwrap();
        for fam in [Family::Debian, Family::Rpm, Family::Unknown] {
            match defs::tool_install_recipe(codex, fam) {
                Recipe::Steps(s) => {
                    let npm = s
                        .iter()
                        .find(|st| st.cmd.contains("npm install -g @openai/codex"))
                        .expect("passo de npm install do codex");
                    assert!(npm.cmd.starts_with("sudo "), "sudo literal no comando: {}", npm.cmd);
                    assert!(npm.sudo, "selo de sudo marcado (guardrail mostra [sudo])");
                }
                _ => panic!("codex sempre tem Steps"),
            }
        }
        // Remoção também com sudo (foi instalado como root).
        match defs::tool_remove_recipe(codex, Family::Debian) {
            Recipe::Steps(s) => {
                assert!(s[0].cmd.starts_with("sudo "));
                assert!(s[0].sudo);
                assert!(s[0].cmd.contains("npm uninstall -g @openai/codex"));
            }
            _ => panic!("codex remove tem Steps"),
        }
    }

    /// Decisão PURA de "precisa fixar o PATH?": só quando o bin NÃO está no PATH mas
    /// EXISTE em ~/.local/bin. Os outros 3 casos não têm PATH a consertar.
    #[test]
    fn needs_path_fix_pura() {
        assert!(needs_path_fix(false, true), "fora do PATH mas em ~/.local/bin → fixa");
        assert!(!needs_path_fix(true, true), "já no PATH → nada a fazer");
        assert!(!needs_path_fix(true, false), "já no PATH → nada a fazer");
        assert!(!needs_path_fix(false, false), "nem no PATH nem local → instalação falhou, não é PATH");
    }

    /// Idempotência PURA: reconhece um rc que já garante ~/.local/bin no PATH (não duplica),
    /// e ignora comentários / linhas irrelevantes.
    #[test]
    fn rc_already_has_local_bin_pura() {
        // Já presente (várias grafias comuns).
        assert!(rc_already_has_local_bin("export PATH=\"$HOME/.local/bin:$PATH\""));
        assert!(rc_already_has_local_bin("export PATH=$PATH:$HOME/.local/bin"));
        assert!(rc_already_has_local_bin(
            "# algo\nfoo=1\nexport PATH=\"$HOME/.local/bin:$PATH\"\nbar=2"
        ));
        // Ausente / não conta.
        assert!(!rc_already_has_local_bin(""));
        assert!(!rc_already_has_local_bin("export PATH=\"$HOME/bin:$PATH\""));
        // Linha comentada NÃO conta (deny-by-default: não confia num export desativado).
        assert!(!rc_already_has_local_bin("# export PATH=\"$HOME/.local/bin:$PATH\""));
        // Menção sem PATH também não conta.
        assert!(!rc_already_has_local_bin("cd $HOME/.local/bin"));
    }

    /// A linha de export canônica satisfaz a própria checagem de idempotência
    /// (garante que, uma vez escrita, não seja re-adicionada numa 2ª execução).
    #[test]
    fn export_line_e_idempotente() {
        assert!(rc_already_has_local_bin(LOCAL_BIN_EXPORT));
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
