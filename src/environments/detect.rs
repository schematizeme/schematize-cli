//! Detecção do ambiente para o engine de environments.
//! O quê: família da distro (via /etc/os-release, como o install.sh), presença de
//! `mise`/`docker`, e se um runtime já está instalado (binário no PATH / mise / imagem).
//! Onde: consumido por `environments::mod` pra decidir métodos disponíveis e idempotência.

use crate::util;

/// Família de distribuição — decide o gerenciador de pacotes do método `distro`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// apt/apt-get (Debian, Ubuntu, Mint).
    Debian,
    /// zypper (openSUSE/SLES) ou dnf (Fedora/RHEL).
    Rpm,
    /// não classificada — o método `distro` fica indisponível (deny-by-default).
    Unknown,
}

impl Family {
    /// Rótulo curto pra logs/tabela.
    pub fn label(self) -> &'static str {
        match self {
            Family::Debian => "debian",
            Family::Rpm => "rpm",
            Family::Unknown => "unknown",
        }
    }
}

/// Lê `/etc/os-release` do sistema (vazio se ausente) e classifica a família.
pub fn family() -> Family {
    family_from(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
}

/// Remove aspas e espaços de um valor de os-release (`ID="fedora"` → `fedora`).
fn unquote(v: &str) -> String {
    v.trim().trim_matches('"').trim_matches('\'').to_string()
}

/// Classifica a família a partir do CONTEÚDO de um os-release — função pura, testável
/// com um arquivo falso. Espelha o `case` do install.sh (ID + ID_LIKE).
pub fn family_from(os_release: &str) -> Family {
    let mut tokens: Vec<String> = Vec::new();
    for line in os_release.lines() {
        let line = line.trim();
        for key in ["ID=", "ID_LIKE="] {
            if let Some(v) = line.strip_prefix(key) {
                for t in unquote(v).split_whitespace() {
                    tokens.push(t.to_lowercase());
                }
            }
        }
    }
    let has = |k: &str| tokens.iter().any(|t| t == k);
    if has("debian") || has("ubuntu") || has("linuxmint") {
        return Family::Debian;
    }
    if has("suse") || has("opensuse") || has("sles") || has("fedora") || has("rhel") || has("centos") {
        return Family::Rpm;
    }
    Family::Unknown
}

/// Um binário está no PATH? (usa `command -v` num shell de login.)
pub fn has_bin(bin: &str) -> bool {
    util::run("bash", &["-lc", &format!("command -v {bin}")])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// `mise` está instalado nesta máquina?
pub fn has_mise() -> bool {
    has_bin("mise")
}

/// `docker` está instalado nesta máquina?
pub fn has_docker() -> bool {
    has_bin("docker")
}

/// O `mise` gerencia (tem instalado) a ferramenta `tool` (ex.: "go", "node")?
pub fn mise_has(tool: &str) -> bool {
    util::run("bash", &["-lc", &format!("mise ls --installed {tool} 2>/dev/null")])
        .map(|s| s.lines().any(|l| !l.trim().is_empty()))
        .unwrap_or(false)
}

/// A imagem docker `img` já está presente localmente? (`docker image inspect`.)
pub fn docker_image_present(img: &str) -> bool {
    util::run("docker", &["image", "inspect", img]).is_ok()
}
