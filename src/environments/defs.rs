//! Definições DATA-DRIVEN dos environments: a tabela dos 7 runtimes e, pra cada
//! (linguagem × método), os comandos CONCRETOS de instalar e de remover.
//! O quê: só DADOS + montadores puros de plano (nenhuma execução aqui). Onde: `mod`
//! consome pra imprimir o plano, pedir consentimento e (só então) executar.
//!
//! Segurança: nenhuma URL inventada — só fontes OFICIAIS conhecidas. O que não tem
//! fonte oficial confiável de instalação sem-interação vira `Recipe::Todo` explícito,
//! nunca um chute.

use super::detect::Family;

/// Um environment de linguagem: runtime + ferramentas comuns de desenvolvimento.
pub struct Env {
    /// slug curto (o que o usuário digita): "go", "rust", ...
    pub lang: &'static str,
    /// nome de exibição na tabela.
    pub display: &'static str,
    /// descrição do runtime instalado.
    pub runtime: &'static str,
    /// ferramentas comuns que o environment provê.
    pub tools: &'static [&'static str],
    /// binário do runtime pra detectar presença no PATH (idempotência).
    pub bin: &'static str,
}

/// A tabela dos 7 environments suportados (fonte de verdade).
pub const ENVS: &[Env] = &[
    Env { lang: "go", display: "Go", runtime: "Go toolchain",
        tools: &["gopls", "golangci-lint", "delve", "goimports"], bin: "go" },
    Env { lang: "rust", display: "Rust", runtime: "rustup (rustc/cargo)",
        tools: &["rust-analyzer", "clippy", "rustfmt"], bin: "cargo" },
    Env { lang: "elixir", display: "Elixir", runtime: "Erlang/OTP + Elixir",
        tools: &["elixir-ls", "hex", "mix"], bin: "elixir" },
    Env { lang: "csharp", display: "C# / .NET", runtime: ".NET SDK",
        tools: &["csharp-ls", "dotnet tools"], bin: "dotnet" },
    Env { lang: "zig", display: "Zig", runtime: "Zig",
        tools: &["zls"], bin: "zig" },
    Env { lang: "ruby", display: "Ruby", runtime: "Ruby",
        tools: &["ruby-lsp", "rubocop", "bundler"], bin: "ruby" },
    Env { lang: "node", display: "Node.js", runtime: "Node.js",
        tools: &["pnpm", "typescript-language-server", "eslint", "prettier"], bin: "node" },
];

/// Resolve um environment pelo slug da linguagem.
pub fn find(lang: &str) -> Option<&'static Env> {
    ENVS.iter().find(|e| e.lang == lang)
}

// ---------------------------------------------------------------------------
// FERRAMENTAS de dev (não-linguagens). Diferente dos runtimes: cada ferramenta
// tem UM caminho canônico de instalação (não os 4 métodos) — o `method` é
// ignorado. Aparecem no MESMO fluxo/UI (detecção por `bin` no PATH, idempotente).
// ---------------------------------------------------------------------------

/// Uma ferramenta de dev com caminho de instalação PRÓPRIO (1 caminho canônico).
pub struct Tool {
    /// slug curto (o que o usuário digita): "claude", "code", "codex".
    pub slug: &'static str,
    /// nome de exibição na tabela.
    pub display: &'static str,
    /// descrição curta (equivalente ao `runtime` do Env).
    pub desc: &'static str,
    /// binário pra detectar presença no PATH (idempotência).
    pub bin: &'static str,
    /// rótulo curto do caminho de instalação (mostrado na coluna "methods").
    pub source_hint: &'static str,
}

/// A tabela das ferramentas de dev suportadas (fonte de verdade).
pub const TOOLS: &[Tool] = &[
    Tool { slug: "claude", display: "Claude Code", desc: "Anthropic Claude Code CLI",
        bin: "claude", source_hint: "official (curl|sh)" },
    Tool { slug: "code", display: "VS Code", desc: "Visual Studio Code",
        bin: "code", source_hint: "distro repo (deb/rpm)" },
    Tool { slug: "codex", display: "Codex CLI", desc: "OpenAI Codex CLI (needs Node.js)",
        bin: "codex", source_hint: "npm -g" },
];

/// Resolve uma ferramenta pelo slug.
pub fn find_tool(slug: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.slug == slug)
}

/// Os 4 métodos de instalação que o usuário escolhe na hora.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    /// imagem oficial da linguagem (isola do host; exige docker).
    Docker,
    /// version manager `mise` (https://mise.run) — `mise use -g <lang>@latest`.
    Mise,
    /// pacotes da distro via apt/dnf/zypper (usa sudo).
    Distro,
    /// instalador que cada comunidade usa (rustup, dotnet-install, fnm, tarball...).
    Official,
}

impl Method {
    /// Todos os métodos, em ordem estável de exibição.
    pub const ALL: [Method; 4] = [Method::Docker, Method::Mise, Method::Distro, Method::Official];

    /// slug do método (o que o usuário passa em --method).
    pub fn slug(self) -> &'static str {
        match self {
            Method::Docker => "docker",
            Method::Mise => "mise",
            Method::Distro => "distro",
            Method::Official => "official",
        }
    }

    /// Faz o parse do --method; None se desconhecido (deny-by-default).
    pub fn parse(s: &str) -> Option<Method> {
        Method::ALL.into_iter().find(|m| m.slug() == s)
    }
}

/// Um passo concreto do plano: o comando EXATO e sua procedência (pra o consentimento).
pub struct Step {
    /// comando de shell exato que será executado.
    pub cmd: String,
    /// procedência humana (de onde vem: mise registry, go.dev, Docker Hub...).
    pub source: String,
    /// exige sudo (o SO vai pedir a senha).
    pub sudo: bool,
    /// baixa e faz PIPE pra um shell (executa código remoto — aviso reforçado).
    pub pipe_sh: bool,
}

/// Resultado de montar um plano pra um (lang × método × família).
pub enum Recipe {
    /// passos executáveis (na ordem).
    Steps(Vec<Step>),
    /// sem fonte oficial confiável nesta combinação — TODO explícito (não chuta).
    Todo(String),
    /// método não se aplica aqui (ex.: distro em família desconhecida).
    Na(String),
}

/// Atalho pra montar um Step.
fn step(cmd: &str, source: &str, sudo: bool, pipe_sh: bool) -> Step {
    Step { cmd: cmd.into(), source: source.into(), sudo, pipe_sh }
}

/// Nome da ferramenta no registro do `mise` pra cada linguagem.
fn mise_tool(lang: &str) -> &'static str {
    match lang {
        "go" => "go",
        "rust" => "rust",
        "elixir" => "elixir",
        "csharp" => "dotnet",
        "zig" => "zig",
        "ruby" => "ruby",
        "node" => "node",
        _ => "",
    }
}

/// Ferramenta do mise pra o método/remoção (Elixir precisa de erlang junto).
pub fn mise_tools(lang: &str) -> Vec<&'static str> {
    if lang == "elixir" {
        vec!["erlang", "elixir"]
    } else {
        vec![mise_tool(lang)]
    }
}

/// Imagem docker OFICIAL da linguagem (tag estável pinada). None = sem imagem oficial.
pub fn docker_image(lang: &str) -> Option<&'static str> {
    match lang {
        "go" => Some("golang:1.23-bookworm"),
        "rust" => Some("rust:1-bookworm"),
        "elixir" => Some("elixir:1.17"),
        "csharp" => Some("mcr.microsoft.com/dotnet/sdk:8.0"),
        "ruby" => Some("ruby:3.3-bookworm"),
        "node" => Some("node:22-bookworm"),
        // Zig não tem imagem OFICIAL no Docker Hub — não inventamos uma de terceiros.
        "zig" => None,
        _ => None,
    }
}

/// Pacotes da distro por (linguagem, gerenciador). Retorna a lista de pacotes do runtime.
/// Nomes podem variar entre distros da mesma família — por isso separamos zypper de dnf.
struct DistroPkgs {
    debian: &'static str,
    zypper: &'static str,
    dnf: &'static str,
}

/// Mapa de pacotes de runtime da distro por linguagem.
fn distro_pkgs(lang: &str) -> Option<DistroPkgs> {
    Some(match lang {
        "go" => DistroPkgs { debian: "golang-go", zypper: "go", dnf: "golang" },
        "rust" => DistroPkgs { debian: "rustc cargo", zypper: "rust cargo", dnf: "rust cargo" },
        "elixir" => DistroPkgs { debian: "elixir", zypper: "elixir", dnf: "elixir" },
        "csharp" => DistroPkgs { debian: "dotnet-sdk-8.0", zypper: "dotnet-sdk-8.0", dnf: "dotnet-sdk-8.0" },
        "zig" => DistroPkgs { debian: "zig", zypper: "zig", dnf: "zig" },
        "ruby" => DistroPkgs { debian: "ruby ruby-dev", zypper: "ruby ruby-devel", dnf: "ruby ruby-devel" },
        "node" => DistroPkgs { debian: "nodejs npm", zypper: "nodejs npm", dnf: "nodejs npm" },
        _ => return None,
    })
}

/// Comando de instalar pacotes conforme a família (embute o if zypper/dnf pra rpm).
fn distro_install_cmd(fam: Family, pkgs: &DistroPkgs) -> String {
    match fam {
        Family::Debian => format!("sudo apt-get update -qq && sudo apt-get install -y {}", pkgs.debian),
        Family::Rpm => format!(
            "if command -v zypper >/dev/null; then sudo zypper --non-interactive install -y {}; \
             else sudo dnf install -y {}; fi",
            pkgs.zypper, pkgs.dnf
        ),
        Family::Unknown => String::new(),
    }
}

/// Comando de remover pacotes conforme a família.
fn distro_remove_cmd(fam: Family, pkgs: &DistroPkgs) -> String {
    match fam {
        Family::Debian => format!("sudo apt-get remove -y {}", pkgs.debian),
        Family::Rpm => format!(
            "if command -v zypper >/dev/null; then sudo zypper --non-interactive rm -y {}; \
             else sudo dnf remove -y {}; fi",
            pkgs.zypper, pkgs.dnf
        ),
        Family::Unknown => String::new(),
    }
}

/// Passos pra instalar as FERRAMENTAS via o pkg-manager da própria linguagem (runtime já no PATH).
/// Reusado por mise/official/distro. `docker` não usa (as ferramentas vivem na imagem).
/// Só inclui o que tem instalador não-interativo confiável; o resto vira `caveats`.
fn tool_steps(lang: &str, method: Method) -> Vec<Step> {
    match lang {
        "go" => vec![
            step("go install golang.org/x/tools/gopls@latest", "go install (proxy.golang.org)", false, false),
            step("go install github.com/golangci/golangci-lint/cmd/golangci-lint@latest", "go install", false, false),
            step("go install github.com/go-delve/delve/cmd/dlv@latest", "go install", false, false),
            step("go install golang.org/x/tools/cmd/goimports@latest", "go install", false, false),
        ],
        "rust" => {
            // rust-analyzer/clippy/rustfmt são COMPONENTES do rustup (mise/official usam rustup);
            // na distro vêm (ou não) como pacotes — por isso só via rustup aqui.
            if method == Method::Distro {
                vec![] // ver caveats: instalar via `rustup component add` ou pacotes da distro
            } else {
                vec![step("rustup component add rust-analyzer clippy rustfmt", "rustup component", false, false)]
            }
        }
        "elixir" => vec![
            step("mix local.hex --force", "hex.pm", false, false),
            step("mix local.rebar --force", "hex.pm", false, false),
        ],
        "csharp" => vec![
            step("dotnet tool install -g csharp-ls", "NuGet (dotnet tool)", false, false),
        ],
        "ruby" => vec![
            step("gem install ruby-lsp rubocop bundler", "rubygems.org", false, false),
        ],
        "node" => vec![
            step("corepack enable && corepack prepare pnpm@latest --activate", "corepack (Node oficial)", false, false),
            step("npm install -g typescript-language-server eslint prettier", "npmjs.com", false, false),
        ],
        _ => vec![],
    }
}

/// Ferramentas que NÃO têm instalação automática confiável nesta combinação — avisadas
/// ao usuário como pendência manual (honestidade: melhor avisar que chutar um instalador).
pub fn tool_caveats(lang: &str, method: Method) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    match lang {
        // elixir-ls não tem instalador oficial de uma linha (o editor costuma buildar do fonte).
        "elixir" => v.push("elixir-ls: instale pelo seu editor (ElixirLS) — sem instalador oficial de 1 comando."),
        // zls acompanha a versão do Zig; via mise há plugin, senão baixe de github.com/zigtools/zls/releases.
        "zig" => {
            if method != Method::Mise {
                v.push("zls: baixe de github.com/zigtools/zls/releases (case a versão com a do Zig).");
            }
        }
        "rust" if method == Method::Distro => {
            v.push("rust-analyzer/clippy/rustfmt: via `rustup component add` ou pacotes da sua distro.");
        }
        _ => {}
    }
    v
}

/// Monta o plano de INSTALAÇÃO pra (env × método × família), sabendo se o mise já existe.
/// Função PURA: só constrói os Steps, nunca executa.
pub fn install_recipe(env: &Env, method: Method, fam: Family, mise_present: bool) -> Recipe {
    match method {
        Method::Docker => match docker_image(env.lang) {
            Some(img) => Recipe::Steps(vec![step(
                &format!("docker pull {img}"),
                "Docker Hub (imagem oficial)",
                false,
                false,
            )]),
            None => Recipe::Todo(format!(
                "sem imagem docker oficial pra {} — não usamos imagens de terceiros.",
                env.display
            )),
        },
        Method::Mise => {
            let mut steps = Vec::new();
            if !mise_present {
                // Instalador oficial do mise: curl | sh (código remoto — aviso reforçado).
                steps.push(step("curl https://mise.run | sh", "https://mise.run", false, true));
            }
            let tools = mise_tools(env.lang).join("@latest ");
            steps.push(step(
                &format!("mise use -g {tools}@latest"),
                "mise registry",
                false,
                false,
            ));
            steps.extend(tool_steps(env.lang, Method::Mise));
            Recipe::Steps(steps)
        }
        Method::Distro => {
            if fam == Family::Unknown {
                return Recipe::Na("família da distro não detectada (/etc/os-release).".into());
            }
            let pkgs = match distro_pkgs(env.lang) {
                Some(p) => p,
                None => return Recipe::Na(format!("sem pacote de distro mapeado pra {}.", env.display)),
            };
            let mut steps = vec![step(&distro_install_cmd(fam, &pkgs), fam.label(), true, false)];
            steps.extend(tool_steps(env.lang, Method::Distro));
            Recipe::Steps(steps)
        }
        Method::Official => official_install(env),
    }
}

/// Instaladores OFICIAIS por comunidade (rustup, tarball do Go, dotnet-install, fnm...).
fn official_install(env: &Env) -> Recipe {
    let mut steps: Vec<Step> = Vec::new();
    match env.lang {
        "go" => {
            // Tarball oficial: pega a versão corrente do endpoint oficial go.dev/VERSION.
            steps.push(step(
                "curl -fsSL \"https://go.dev/dl/$(curl -fsSL https://go.dev/VERSION?m=text | head -1).linux-amd64.tar.gz\" -o /tmp/schematize-go.tar.gz \
                 && sudo rm -rf /usr/local/go && sudo tar -C /usr/local -xzf /tmp/schematize-go.tar.gz",
                "go.dev/dl (oficial)",
                true,
                false,
            ));
            steps.push(note_step("adicione /usr/local/go/bin ao PATH (ex.: em ~/.profile)."));
        }
        "rust" => {
            steps.push(step(
                "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y",
                "https://sh.rustup.rs (rustup oficial)",
                false,
                true,
            ));
        }
        "csharp" => {
            steps.push(step(
                "curl -fsSL https://dot.net/v1/dotnet-install.sh | bash -s -- --channel LTS",
                "https://dot.net (dotnet-install oficial)",
                false,
                true,
            ));
        }
        "node" => {
            steps.push(step(
                "curl -fsSL https://fnm.vercel.app/install | bash",
                "https://fnm.vercel.app (fnm oficial)",
                false,
                true,
            ));
            steps.push(step("fnm install --lts && fnm default lts-latest", "fnm", false, false));
        }
        // Sem instalador oficial de UMA linha e sem-interação confiável: TODO honesto.
        "zig" => {
            return Recipe::Todo(
                "Zig oficial: baixe o tarball de https://ziglang.org/download/ (sem instalador único). \
                 Prefira `--method mise` ou `--method distro`."
                    .into(),
            )
        }
        "ruby" => {
            return Recipe::Todo(
                "Ruby oficial usa ruby-build/ruby-install (exige instalar o builder antes). \
                 Prefira `--method mise` ou `--method distro`."
                    .into(),
            )
        }
        "elixir" => {
            return Recipe::Todo(
                "Erlang/Elixir não têm um instalador oficial único (a comunidade usa asdf/mise/kerl+kiex). \
                 Prefira `--method mise` ou `--method distro`."
                    .into(),
            )
        }
        _ => return Recipe::Na("linguagem desconhecida.".into()),
    }
    steps.extend(tool_steps(env.lang, Method::Official));
    Recipe::Steps(steps)
}

/// Passo puramente informativo (mostra uma orientação; não executa nada — usa `true`/no-op).
fn note_step(msg: &str) -> Step {
    Step { cmd: format!(": # {msg}"), source: "nota".into(), sudo: false, pipe_sh: false }
}

/// Monta o plano de REMOÇÃO pra (env × método × família).
pub fn remove_recipe(env: &Env, method: Method, fam: Family) -> Recipe {
    match method {
        Method::Docker => match docker_image(env.lang) {
            Some(img) => Recipe::Steps(vec![step(&format!("docker rmi {img}"), "docker", false, false)]),
            None => Recipe::Na(format!("sem imagem docker oficial pra {}.", env.display)),
        },
        Method::Mise => {
            let tools = mise_tools(env.lang).join(" ");
            Recipe::Steps(vec![step(&format!("mise uninstall {tools}"), "mise", false, false)])
        }
        Method::Distro => {
            if fam == Family::Unknown {
                return Recipe::Na("família da distro não detectada.".into());
            }
            match distro_pkgs(env.lang) {
                Some(p) => Recipe::Steps(vec![step(&distro_remove_cmd(fam, &p), fam.label(), true, false)]),
                None => Recipe::Na(format!("sem pacote de distro mapeado pra {}.", env.display)),
            }
        }
        Method::Official => match env.lang {
            "go" => Recipe::Steps(vec![step("sudo rm -rf /usr/local/go", "go.dev", true, false)]),
            "rust" => Recipe::Steps(vec![step("rustup self uninstall -y", "rustup", false, false)]),
            "csharp" => Recipe::Steps(vec![step("rm -rf \"$HOME/.dotnet\"", "dotnet-install", false, false)]),
            "node" => Recipe::Steps(vec![step("rm -rf \"$HOME/.local/share/fnm\"", "fnm", false, false)]),
            _ => Recipe::Todo(format!(
                "remoção oficial de {} não definida (instalação oficial é TODO nesta combinação).",
                env.display
            )),
        },
    }
}

// ---------------------------------------------------------------------------
// Planos das FERRAMENTAS. Cada uma tem 1 caminho canônico; o VS Code varia por
// família (deb vs rpm/repo). Funções PURAS: só montam Steps, nunca executam.
// ---------------------------------------------------------------------------

/// Monta o plano de INSTALAÇÃO de uma ferramenta (varia por família só no VS Code).
pub fn tool_install_recipe(tool: &Tool, fam: Family) -> Recipe {
    match tool.slug {
        // Instalador oficial (sem sudo, vai pra ~/.local/bin). curl | bash = código remoto.
        "claude" => Recipe::Steps(vec![step(
            "curl -fsSL https://claude.ai/install.sh | bash",
            "https://claude.ai/install.sh (instalador oficial Anthropic)",
            false,
            true,
        )]),
        // VS Code: .deb oficial no apt; repo oficial da Microsoft no rpm (dnf/zypper).
        "code" => match fam {
            Family::Debian => Recipe::Steps(vec![step(
                "curl -fsSL \"https://code.visualstudio.com/sha/download?build=stable&os=linux-deb-x64\" \
                 -o /tmp/schematize-vscode.deb && sudo apt-get install -y /tmp/schematize-vscode.deb",
                "code.visualstudio.com (.deb oficial)",
                true,
                false,
            )]),
            Family::Rpm => Recipe::Steps(vec![
                step(
                    "sudo rpm --import https://packages.microsoft.com/keys/microsoft.asc",
                    "packages.microsoft.com (chave oficial)",
                    true,
                    false,
                ),
                step(
                    "if command -v zypper >/dev/null; then \
                       sudo zypper --non-interactive addrepo -f https://packages.microsoft.com/yumrepos/vscode vscode; \
                     else \
                       printf '[code]\\nname=Visual Studio Code\\nbaseurl=https://packages.microsoft.com/yumrepos/vscode\\nenabled=1\\nautorefresh=1\\ntype=rpm-md\\ngpgcheck=1\\ngpgkey=https://packages.microsoft.com/keys/microsoft.asc\\n' | sudo tee /etc/yum.repos.d/vscode.repo >/dev/null; \
                     fi",
                    "packages.microsoft.com/yumrepos/vscode (repo oficial)",
                    true,
                    false,
                ),
                step(
                    "if command -v zypper >/dev/null; then sudo zypper --non-interactive install -y code; \
                     else sudo dnf install -y code; fi",
                    "repo oficial da Microsoft",
                    true,
                    false,
                ),
            ]),
            Family::Unknown => Recipe::Na(
                "família da distro não detectada — VS Code precisa de apt (.deb) ou dnf/zypper (rpm).".into(),
            ),
        },
        // Codex CLI via npm global (depende de Node.js/npm — cita a dependência).
        "codex" => Recipe::Steps(vec![
            note_step("requer Node.js/npm no PATH (instale antes: `schematize env install node`)."),
            step(
                "npm install -g @openai/codex",
                "npmjs.com (@openai/codex)",
                false,
                false,
            ),
        ]),
        _ => Recipe::Na(format!("ferramenta desconhecida: {}.", tool.slug)),
    }
}

/// Monta o plano de REMOÇÃO de uma ferramenta (varia por família só no VS Code).
pub fn tool_remove_recipe(tool: &Tool, fam: Family) -> Recipe {
    match tool.slug {
        // O instalador oficial não expõe uninstall documentado — remove o binário do ~/.local/bin.
        "claude" => Recipe::Steps(vec![step(
            "rm -f \"$HOME/.local/bin/claude\"",
            "instalador oficial (sem uninstall — remove o binário)",
            false,
            false,
        )]),
        "code" => match fam {
            Family::Debian => Recipe::Steps(vec![step(
                "sudo apt-get remove -y code",
                fam.label(),
                true,
                false,
            )]),
            Family::Rpm => Recipe::Steps(vec![step(
                "if command -v zypper >/dev/null; then sudo zypper --non-interactive rm -y code; \
                 else sudo dnf remove -y code; fi",
                fam.label(),
                true,
                false,
            )]),
            Family::Unknown => Recipe::Na("família da distro não detectada.".into()),
        },
        "codex" => Recipe::Steps(vec![step(
            "npm uninstall -g @openai/codex",
            "npmjs.com (@openai/codex)",
            false,
            false,
        )]),
        _ => Recipe::Na(format!("ferramenta desconhecida: {}.", tool.slug)),
    }
}
