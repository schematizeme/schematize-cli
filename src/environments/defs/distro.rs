//! DEFS/DISTRO — o método `distro`: nomes de pacote e comandos, por FAMÍLIA.
//!
//! **O quê:** o mapa `linguagem × família → pacotes`, os comandos de instalar/remover de cada
//! gerenciador, e a checagem de disponibilidade que recusa antes de pedir sudo.
//!
//! **Onde:** `defs::install_recipe` e `defs::remove_recipe`.
//!
//! ## Por que virou arquivo próprio
//!
//! O `defs.rs` passou de 750 linhas ao ganhar o Arch e a checagem de disponibilidade, e o
//! piso §6 da casa reprovou — corretamente. O corte é por COESÃO, não por tamanho: tudo aqui
//! responde a uma pergunta só — *"como esta família instala software?"* —, e nada mais no
//! `defs` precisa saber disso.
//!
//! ## Campo vazio é INFORMAÇÃO, não esquecimento
//!
//! Quando o pacote de uma família está vazio, isso significa **não existe nos repos padrão
//! dela**. É o que faz [`distro_indisponivel`] recusar com uma saída, em vez de montar um
//! `install` que morre com "Nenhum fornecedor encontrado" depois de a pessoa digitar a senha.

use super::Family;

/// Pacotes da distro por (linguagem, gerenciador). Retorna a lista de pacotes do runtime.
/// Nomes podem variar entre distros da mesma família — por isso separamos zypper de dnf.
pub(super) struct DistroPkgs {
    debian: &'static str,
    /// openSUSE/SLES. Vazio = **não existe nos repos padrão desta família**.
    zypper: &'static str,
    /// Fedora/RHEL. Vazio = idem.
    dnf: &'static str,
    /// Arch e derivadas. Vazio = idem.
    arch: &'static str,
}

/// Mapa de pacotes de runtime da distro por linguagem.
pub(super) fn distro_pkgs(lang: &str) -> Option<DistroPkgs> {
    Some(match lang {
        "go" => DistroPkgs { debian: "golang-go", zypper: "go", dnf: "golang", arch: "go" },
        "rust" => DistroPkgs {
            debian: "rustc cargo",
            zypper: "rust cargo",
            dnf: "rust cargo",
            arch: "rust",
        },
        "elixir" => {
            DistroPkgs { debian: "elixir", zypper: "elixir", dnf: "elixir", arch: "elixir" }
        }
        // O .NET é o caso que expôs o buraco: NÃO está nos repos padrão do openSUSE (medido:
        // `zypper search dotnet` devolve zero no Leap 16). O vazio aqui não é esquecimento —
        // é a informação de que essa via não existe, e é o que faz a ferramenta avisar em vez
        // de rodar um `zypper install` que falha com "exit 104".
        "csharp" => DistroPkgs {
            debian: "dotnet-sdk-8.0",
            zypper: "",
            dnf: "dotnet-sdk-8.0",
            arch: "dotnet-sdk",
        },
        "zig" => DistroPkgs { debian: "zig", zypper: "zig", dnf: "zig", arch: "zig" },
        "ruby" => DistroPkgs {
            debian: "ruby ruby-dev",
            zypper: "ruby ruby-devel",
            dnf: "ruby ruby-devel",
            arch: "ruby",
        },
        "node" => DistroPkgs {
            debian: "nodejs npm",
            zypper: "nodejs npm",
            dnf: "nodejs npm",
            arch: "nodejs npm",
        },
        _ => return None,
    })
}

/// Comando de instalar pacotes conforme a família (embute o if zypper/dnf pra rpm).
pub(super) fn distro_install_cmd(fam: Family, pkgs: &DistroPkgs) -> String {
    // Cada família tem o SEU comando. Antes havia um `if command -v zypper …` embutido, porque
    // Fedora e SUSE eram a mesma `Family::Rpm` — e o usuário via aquele shell no plano de
    // consentimento, sem saber qual metade rodaria na máquina dele.
    match (fam, pkgs_da_familia(fam, pkgs)) {
        (_, "") => String::new(), // não há pacote nesta família — ver `distro_indisponivel`
        (Family::Debian, p) => format!("sudo apt-get update -qq && sudo apt-get install -y {p}"),
        (Family::Suse, p) => format!("sudo zypper --non-interactive install -y {p}"),
        (Family::Fedora, p) => format!("sudo dnf install -y {p}"),
        (Family::Arch, p) => format!("sudo pacman -S --noconfirm --needed {p}"),
        (Family::Unknown, _) => String::new(),
    }
}

/// **O quê:** os pacotes desta família. Vazio = a linguagem não existe nos repos padrão dela.
/// **Onde:** os comandos de instalar/remover e a checagem de disponibilidade.
pub(super) fn pkgs_da_familia(fam: Family, p: &DistroPkgs) -> &'static str {
    match fam {
        Family::Debian => p.debian,
        Family::Suse => p.zypper,
        Family::Fedora => p.dnf,
        Family::Arch => p.arch,
        Family::Unknown => "",
    }
}

/// Comando de remover pacotes conforme a família.
pub(super) fn distro_remove_cmd(fam: Family, pkgs: &DistroPkgs) -> String {
    match fam {
        _ if pkgs_da_familia(fam, pkgs).is_empty() => String::new(),
        Family::Debian => format!("sudo apt-get remove -y {}", pkgs.debian),
        Family::Suse => format!("sudo zypper --non-interactive rm -y {}", pkgs.zypper),
        Family::Fedora => format!("sudo dnf remove -y {}", pkgs.dnf),
        // `-Rns`: leva as dependências que ficaram órfãs e os arquivos de config do pacote.
        // Sem o `s`, desinstalar deixa um rastro que ninguém depois sabe de onde veio.
        Family::Arch => format!("sudo pacman -Rns --noconfirm {}", pkgs.arch),
        Family::Unknown => String::new(),
    }
}

/// **O quê:** a linguagem existe nos repos padrão desta família? Se não, o porquê e a saída.
///
/// **Onde:** o planejador, ANTES de montar os passos. Função PURA.
///
/// **Por que existe:** sem ela, `env install csharp --method distro` num openSUSE montava um
/// `zypper install dotnet-sdk-8.0`, pedia sudo, e morria com
/// `Nenhum fornecedor encontrado` / `exit status: 104`. A pessoa digitava a senha para ver um
/// erro do gerenciador de pacotes que não diz o que fazer.
///
/// Recusar **antes** custa nada e diz o próximo passo — §37.48. A informação já estava na
/// tabela (o campo vazio); só faltava alguém perguntar.
pub fn distro_indisponivel(lang: &str, fam: Family) -> Option<String> {
    let pkgs = distro_pkgs(lang)?;
    if !pkgs_da_familia(fam, &pkgs).is_empty() {
        return None;
    }
    let alternativas = "mise (recomendado), official ou docker";
    Some(match (lang, fam) {
        ("csharp", Family::Suse) => String::from(
            "o .NET NÃO está nos repositórios padrão do openSUSE/SLES — a Microsoft o publica \
             no repo dela.\n  Use outro método: `schematize env install csharp --method mise`\n               (ou, se quiser mesmo o repo da Microsoft, adicione-o você: \
             https://learn.microsoft.com/dotnet/core/install/linux-opensuse)"
        ),
        _ => format!(
            "`{lang}` não está nos repositórios padrão desta distro ({}).\n  Use outro método: \
             {alternativas} — ex.: `schematize env install {lang} --method mise`",
            fam.label()
        ),
    })
}

#[cfg(test)]
mod tests_disponibilidade {
    use super::*;

    /// **O caso de campo.** `.NET` não está nos repos do openSUSE — medido: `zypper search
    /// dotnet` devolve zero no Leap 16. Sem esta recusa, a pessoa digitava a senha do sudo
    /// para ver `Nenhum fornecedor encontrado` / `exit 104`.
    #[test]
    fn csharp_no_suse_recusa_antes_de_pedir_sudo() {
        let m = distro_indisponivel("csharp", Family::Suse).expect("devia recusar");
        assert!(m.contains("--method mise"), "tem de dar a saída: {m}");
        assert!(m.contains("Microsoft"), "tem de dizer POR QUE não está lá: {m}");
    }

    /// E onde ele existe, não recusa — a guarda não pode virar um "não" genérico.
    #[test]
    fn csharp_passa_onde_o_pacote_existe() {
        for f in [Family::Debian, Family::Fedora, Family::Arch] {
            assert!(distro_indisponivel("csharp", f).is_none(), "{f:?} tem dotnet nos repos");
        }
    }

    /// As linguagens que existem em todas as quatro seguem passando em todas.
    #[test]
    fn o_comum_continua_disponivel_nas_quatro() {
        for lang in ["go", "rust", "ruby", "node", "zig", "elixir"] {
            for f in [Family::Debian, Family::Fedora, Family::Suse, Family::Arch] {
                assert!(distro_indisponivel(lang, f).is_none(), "{lang} devia existir em {f:?}");
            }
        }
    }

    /// **Cada família gera o comando DELA** — sem `if command -v` em tempo de execução, que
    /// era o que aparecia no plano de consentimento sem dizer qual metade rodaria.
    #[test]
    fn cada_familia_tem_o_gerenciador_certo() {
        let p = distro_pkgs("go").unwrap();
        assert!(distro_install_cmd(Family::Debian, &p).contains("apt-get"));
        assert!(distro_install_cmd(Family::Suse, &p).contains("zypper"));
        assert!(distro_install_cmd(Family::Fedora, &p).contains("dnf"));
        assert!(distro_install_cmd(Family::Arch, &p).contains("pacman"));
        for f in [Family::Debian, Family::Suse, Family::Fedora, Family::Arch] {
            let c = distro_install_cmd(f, &p);
            assert!(!c.contains("command -v"), "{f:?} ainda decide em runtime: {c}");
        }
    }

    /// Pacote ausente na família gera comando VAZIO — nunca um `install` sem argumento, que
    /// no apt abre o modo interativo e no pacman tenta instalar o repositório inteiro.
    #[test]
    fn familia_sem_pacote_nao_gera_comando() {
        let p = distro_pkgs("csharp").unwrap();
        assert_eq!(distro_install_cmd(Family::Suse, &p), "", "comando vazio, não `install `");
        assert_eq!(distro_remove_cmd(Family::Suse, &p), "");
    }
}
