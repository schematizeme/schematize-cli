//! Sonda da MÁQUINA: o que existe aqui (gestor de pacote, docker, mise) e qual
//! método de instalação já está em uso para cada linguagem.

use super::*;

/// Snapshot do que a máquina oferece — calculado uma vez por comando.
pub(crate) struct Machine {
    pub(crate) family: Family,
    pub(crate) mise: bool,
    pub(crate) docker: bool,
}

impl Machine {
    /// Sonda a máquina (família da distro, mise/docker presentes).
    pub(crate) fn probe() -> Machine {
        Machine { family: detect::family(), mise: detect::has_mise(), docker: detect::has_docker() }
    }

    /// Métodos DISPONÍVEIS nesta máquina, em ordem estável.
    /// mise/official sempre disponíveis (bootstrappáveis); docker só com docker; distro só com família.
    pub(crate) fn available(&self) -> Vec<Method> {
        Method::ALL.into_iter().filter(|m| self.method_reason(*m).is_ok()).collect()
    }

    /// Ok se o método é utilizável aqui; Err(razão) caso contrário (deny-by-default).
    pub(crate) fn method_reason(&self, m: Method) -> Result<(), String> {
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
pub(crate) fn installed_method(env: &Env, m: &Machine) -> Option<Method> {
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

/// Idempotência: environment já satisfeito por este método?
pub(crate) fn already_installed(env: &Env, method: Method, m: &Machine) -> bool {
    match method {
        Method::Docker => {
            defs::docker_image(env.lang).map(detect::docker_image_present).unwrap_or(false)
        }
        Method::Mise => {
            m.mise && detect::mise_has(defs::mise_tools(env.lang).last().copied().unwrap_or(""))
        }
        // distro/official: se o runtime já está no PATH, o objetivo do env está atendido.
        Method::Distro | Method::Official => detect::has_bin(env.bin),
    }
}
