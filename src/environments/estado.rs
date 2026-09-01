//! ESTADO por linguagem/ferramenta: o que está instalado, por qual método, e a
//! renderização da tabela do `schematize env list`.

use super::*;

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
pub(crate) fn status_text(le: &LangEnv) -> String {
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
    println!("  {:<14} {:<34} {}", t("env.col_lang"), t("env.col_methods"), t("env.col_status"));
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
