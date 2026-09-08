//! PROCEDÊNCIA — descobrir **por onde** um runtime foi instalado, não só que ele existe.
//!
//! **O quê:** dado o binário de uma linguagem, diz se ele veio do `mise`, de uma imagem
//! docker, de um pacote da distro, do instalador oficial da comunidade — ou de lugar que não
//! se consegue atribuir.
//!
//! **Onde:** `env list` (a coluna de status) e `env switch` (para saber o que desfazer).
//!
//! ## O buraco que isto fecha
//!
//! A detecção anterior só reconhecia **docker** e **mise**. Para tudo o mais ela devolvia
//! `None`, e a tabela dizia apenas "instalado" — então `Go` aparecia como "via mise" e `Rust`
//! como "instalado", sem que nada dissesse por onde o Rust tinha entrado. Quem quisesse
//! trocar de método não tinha como saber do que estava saindo.
//!
//! ## Como se distingue distro de oficial, sem adivinhar
//!
//! Resolvendo o caminho ABSOLUTO do binário e perguntando ao gerenciador de pacotes **quem é
//! o dono daquele arquivo** — `rpm -qf`, `dpkg -S`. Se um pacote o reivindica, é `distro`. Se
//! nenhum reivindica e o caminho é de um instalador conhecido (`~/.cargo/bin` do rustup,
//! `~/.dotnet`), é `official`.
//!
//! ## O quinto estado, e por que ele existe
//!
//! Se o binário está lá e **nada** o explica — instalado à mão, por `asdf`, por `nvm`, por um
//! tarball que alguém descompactou em 2019 —, a resposta é [`Procedencia::Desconhecida`], e
//! **não** "official".
//!
//! Chamar de oficial o que não se sabe seria dar ao usuário uma certeza falsa, e ele agiria
//! sobre ela: um `switch` que acredita ter vindo do rustup vai tentar `rustup self uninstall`
//! num binário que o rustup nunca viu. Dizer "não sei de onde veio" é menos satisfatório e
//! muito mais útil.

use super::{defs, detect};
use std::path::{Path, PathBuf};

/// Por onde um runtime entrou nesta máquina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Procedencia {
    /// Gerido pelo `mise`.
    Mise,
    /// Há imagem docker da linguagem presente (não há binário no host).
    Docker,
    /// Um pacote da distro é dono do arquivo. Traz o nome do pacote.
    Distro { pacote: String },
    /// Instalador da comunidade (rustup, dotnet-install…). Traz o caminho que o denuncia.
    Oficial { caminho: PathBuf },
    /// O binário existe e **nada** o explica. Ver a nota no topo do módulo.
    Desconhecida { caminho: PathBuf },
    /// Não está instalado por via nenhuma.
    Ausente,
}

impl Procedencia {
    /// **O quê:** o texto curto da tabela.
    /// **Onde:** `env list`. Diz o que se sabe, e **admite** o que não se sabe.
    pub fn rotulo(&self) -> String {
        match self {
            Procedencia::Mise => "via mise".into(),
            Procedencia::Docker => "via docker".into(),
            Procedencia::Distro { pacote } => format!("via distro ({pacote})"),
            Procedencia::Oficial { .. } => "via instalador oficial".into(),
            Procedencia::Desconhecida { caminho } => {
                format!("origem desconhecida ({})", caminho.display())
            }
            Procedencia::Ausente => "não instalado".into(),
        }
    }

    /// **O quê:** o método equivalente, quando há um.
    ///
    /// **Onde:** `env switch`, para saber o que remover. `Desconhecida` devolve `None` de
    /// propósito — não se remove pelo método errado o que não se sabe de onde veio.
    pub fn metodo(&self) -> Option<defs::Method> {
        match self {
            Procedencia::Mise => Some(defs::Method::Mise),
            Procedencia::Docker => Some(defs::Method::Docker),
            Procedencia::Distro { .. } => Some(defs::Method::Distro),
            Procedencia::Oficial { .. } => Some(defs::Method::Official),
            Procedencia::Desconhecida { .. } | Procedencia::Ausente => None,
        }
    }

    /// **O quê:** está instalado, por qualquer via?
    pub fn instalado(&self) -> bool {
        !matches!(self, Procedencia::Ausente)
    }
}

/// **O quê:** o pacote da distro dono de `caminho`, se houver.
///
/// **Onde:** [`de`]. É a pergunta que distingue `distro` de `official` sem adivinhar: quem
/// instalou o arquivo, o gerenciador de pacotes ou outra coisa?
///
/// Devolve `None` quando nenhum pacote o reivindica **ou** quando não há gerenciador que
/// saiba responder — nos dois casos a conclusão é a mesma: não veio da distro.
pub fn dono_do_arquivo(caminho: &Path) -> Option<String> {
    let p = caminho.to_string_lossy();
    // rpm: `rpm -qf <path>` → nome do pacote, ou erro se não é de pacote nenhum.
    if let Ok(saida) = crate::util::run("rpm", &["-qf", "--queryformat", "%{NAME}", &p]) {
        let s = saida.trim();
        if !s.is_empty() && !s.contains("not owned") {
            return Some(s.to_string());
        }
    }
    // dpkg: `dpkg -S <path>` → "pacote: /caminho".
    if let Ok(saida) = crate::util::run("dpkg", &["-S", &p]) {
        if let Some(nome) = saida.split(':').next() {
            let n = nome.trim();
            if !n.is_empty() && !n.contains("no path found") {
                return Some(n.to_string());
            }
        }
    }
    None
}

/// **O quê:** o caminho denuncia um instalador oficial conhecido?
///
/// **Onde:** [`de`], só depois de o gerenciador de pacotes ter dito que não é dele.
///
/// Função PURA sobre o caminho — a lista é curta e cada entrada é um instalador que a
/// comunidade daquela linguagem de fato usa. Fora dela, a resposta é "não sei".
pub fn caminho_de_instalador_oficial(caminho: &Path) -> bool {
    let p = caminho.to_string_lossy().replace('\\', "/");
    // rustup, dotnet-install, fnm/volta, sdkman, go oficial, bun.
    const MARCAS: &[&str] = &[
        "/.cargo/bin/",
        "/.rustup/",
        "/.dotnet/",
        "/.fnm/",
        "/.volta/",
        "/.sdkman/",
        "/usr/local/go/",
        "/.bun/",
        "/.deno/",
    ];
    MARCAS.iter().any(|m| p.contains(m))
}

/// **O quê:** a procedência de uma linguagem nesta máquina.
///
/// **Onde:** `env list` e `env switch`.
///
/// **A ordem das perguntas importa.** `mise` e `docker` primeiro, porque são afirmações
/// POSITIVAS e baratas: o `mise` diz que gerencia, o docker diz que tem a imagem. Só depois
/// se vai ao binário no PATH — que é o terreno onde distro e oficial se confundem, e onde a
/// única resposta honesta vem do dono do arquivo.
pub fn de(lang: &str, bin: &str, tem_mise: bool, tem_docker: bool) -> Procedencia {
    if tem_mise && detect::mise_has(defs::mise_tools(lang).last().copied().unwrap_or("")) {
        return Procedencia::Mise;
    }
    if tem_docker {
        if let Some(img) = defs::docker_image(lang) {
            if detect::docker_image_present(img) {
                return Procedencia::Docker;
            }
        }
    }
    let Some(caminho) = crate::agentrun::resolve_bin(bin) else {
        return Procedencia::Ausente;
    };
    // Resolve link simbólico: `/usr/bin/go` pode apontar para o arquivo que o pacote possui,
    // e perguntar pelo link daria "não é de pacote nenhum" — uma resposta errada.
    let real = std::fs::canonicalize(&caminho).unwrap_or_else(|_| caminho.clone());
    if let Some(pacote) = dono_do_arquivo(&real) {
        return Procedencia::Distro { pacote };
    }
    if caminho_de_instalador_oficial(&real) {
        return Procedencia::Oficial { caminho: real };
    }
    Procedencia::Desconhecida { caminho: real }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O rótulo diz o que se sabe — e, quando não se sabe, **diz isso**, com o caminho para a
    /// pessoa investigar. "Instalado" mudo era exatamente o problema que este módulo veio
    /// resolver.
    #[test]
    fn o_rotulo_admite_o_que_nao_sabe() {
        assert_eq!(Procedencia::Mise.rotulo(), "via mise");
        let d = Procedencia::Distro { pacote: "golang-bin".into() };
        assert!(d.rotulo().contains("golang-bin"), "o pacote tem de aparecer: {}", d.rotulo());
        let x = Procedencia::Desconhecida { caminho: "/opt/velho/bin/go".into() };
        assert!(x.rotulo().contains("desconhecida"), "{}", x.rotulo());
        assert!(x.rotulo().contains("/opt/velho"), "sem o caminho não dá para investigar");
    }

    /// **A regra que impede o estrago:** origem desconhecida NÃO tem método.
    ///
    /// Um `switch` que acreditasse ter vindo do rustup tentaria `rustup self uninstall` num
    /// binário que o rustup nunca viu — e falharia, ou pior, removeria outra coisa.
    #[test]
    fn desconhecida_nao_tem_metodo() {
        let x = Procedencia::Desconhecida { caminho: "/opt/x/bin/go".into() };
        assert_eq!(x.metodo(), None, "sem método: não se desfaz o que não se sabe como foi feito");
        assert!(x.instalado(), "mas ESTÁ instalado — as duas coisas são independentes");
        assert_eq!(Procedencia::Ausente.metodo(), None);
        assert!(!Procedencia::Ausente.instalado());
    }

    /// Cada procedência conhecida mapeia no método que a desfaz.
    #[test]
    fn procedencia_conhecida_vira_metodo() {
        assert_eq!(Procedencia::Mise.metodo(), Some(defs::Method::Mise));
        assert_eq!(Procedencia::Docker.metodo(), Some(defs::Method::Docker));
        assert_eq!(Procedencia::Distro { pacote: "x".into() }.metodo(), Some(defs::Method::Distro));
        assert_eq!(
            Procedencia::Oficial { caminho: "/c".into() }.metodo(),
            Some(defs::Method::Official)
        );
    }

    /// Os caminhos que denunciam instalador oficial — e os que NÃO denunciam.
    ///
    /// A lista é curta de propósito: fora dela a resposta é "não sei", que é melhor que um
    /// palpite que o usuário vai tratar como certeza.
    #[test]
    fn reconhece_instalador_oficial_e_recusa_o_resto() {
        for bom in [
            "/home/u/.cargo/bin/cargo",
            "/home/u/.rustup/toolchains/stable/bin/rustc",
            "/home/u/.dotnet/dotnet",
            "/usr/local/go/bin/go",
            "/home/u/.bun/bin/bun",
        ] {
            assert!(caminho_de_instalador_oficial(Path::new(bom)), "devia reconhecer {bom}");
        }
        for nao in [
            "/usr/bin/go",              // isso é da distro, e o rpm/dpkg que diz
            "/opt/manual/bin/node",     // alguém descompactou aqui
            "/home/u/.asdf/shims/ruby", // outro version manager: não é "oficial"
            "/home/u/bin/zig",
        ] {
            assert!(!caminho_de_instalador_oficial(Path::new(nao)), "NÃO devia reconhecer {nao}");
        }
    }

    /// `~/.asdf` é o caso que mais tenta: é um gerenciador, mas não é o `mise` nem um
    /// instalador oficial. A resposta certa é "desconhecida" — e é o que faz o `switch`
    /// recusar em vez de rodar o comando errado.
    #[test]
    fn outro_version_manager_nao_vira_oficial() {
        assert!(!caminho_de_instalador_oficial(Path::new("/home/u/.asdf/shims/node")));
        assert!(!caminho_de_instalador_oficial(Path::new(
            "/home/u/.nvm/versions/node/v20/bin/node"
        )));
    }
}
