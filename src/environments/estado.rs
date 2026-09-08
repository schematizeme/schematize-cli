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
    /// POR ONDE entrou — não só que entrou. `None` para ferramentas (têm um caminho só).
    ///
    /// O campo `installed` acima só sabia de docker e mise; para distro e oficial ele dizia
    /// `None`, e a tabela mostrava "instalado" mudo. Este responde a pergunta que faltava.
    pub procedencia: Option<super::procedencia::Procedencia>,
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
        procedencia: Some(super::procedencia::de(env.lang, env.bin, m.mise, m.docker)),
    });
    let tools = defs::TOOLS.iter().map(|tool| LangEnv {
        lang: tool.slug,
        display: tool.display,
        category: "tool",
        install_hint: tool.source_hint.to_string(),
        methods_available: Vec::new(),
        installed: None,
        runtime_present: detect::has_bin(tool.bin),
        // Ferramenta tem UM caminho canônico; perguntar "por onde" não faz sentido.
        procedencia: None,
    });
    langs.chain(tools).collect()
}

/// Texto de status pra a tabela: instalado por qual método, ou só "instalado", ou não.
pub(crate) fn status_text(le: &LangEnv) -> String {
    // A PROCEDÊNCIA manda quando existe: ela distingue as quatro vias, e diz "não sei" em vez
    // de deixar um "instalado" mudo. O `installed` (docker/mise) fica como retaguarda para
    // quem consome a struct e ainda não conhece o campo novo.
    if let Some(p) = &le.procedencia {
        if p.instalado() {
            return p.rotulo();
        }
    }
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

#[cfg(test)]
mod tests_procedencia {
    use super::*;
    use crate::environments::procedencia::Procedencia;

    fn le(p: Option<Procedencia>, runtime: bool) -> LangEnv {
        LangEnv {
            lang: "x",
            display: "X",
            category: "language",
            install_hint: String::new(),
            methods_available: Vec::new(),
            installed: None,
            runtime_present: runtime,
            procedencia: p,
        }
    }

    /// **O buraco que isto fechou:** antes, distro e official caíam num "instalado" mudo —
    /// o runtime estava lá e nada dizia por onde entrou. Agora o rótulo diz.
    #[test]
    fn distro_e_oficial_deixam_de_ser_instalado_mudo() {
        let d = le(Some(Procedencia::Distro { pacote: "ruby3.4".into() }), true);
        assert!(status_text(&d).contains("ruby3.4"), "{}", status_text(&d));

        let o = le(Some(Procedencia::Oficial { caminho: "/home/u/.cargo/bin/cargo".into() }), true);
        assert!(status_text(&o).contains("oficial"), "{}", status_text(&o));
    }

    /// Origem desconhecida é DITA, não maquiada de "instalado" — e traz o caminho, que é o
    /// que permite investigar.
    #[test]
    fn origem_desconhecida_aparece_na_tabela() {
        let x = le(Some(Procedencia::Desconhecida { caminho: "/opt/velho/bin/node".into() }), true);
        let t = status_text(&x);
        assert!(t.contains("desconhecida"), "{t}");
        assert!(t.contains("/opt/velho"), "sem o caminho não dá para investigar: {t}");
    }

    /// Ferramenta não tem procedência (um caminho canônico só) — e o texto continua o de
    /// antes, sem regressão.
    #[test]
    fn ferramenta_sem_procedencia_mantem_o_texto_antigo() {
        let t = le(None, true);
        assert!(!status_text(&t).contains("via "), "ferramenta não tem 'via': {}", status_text(&t));
    }
}
