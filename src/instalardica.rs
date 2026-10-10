//! A dica de "como instalar" de cada app da casa — a que FUNCIONA, não a que seria bonita.
//!
//! **O quê:** para cada app que o hub delega, registra (a) se o `schematize-market` o gere e
//! (b) se o repositório é público, e devolve o comando de instalação que de fato resolve.
//!
//! **Onde:** [`crate::deployerlink::como_instalar_app`], que alimenta a mensagem de "app
//! ausente" (`applink::interceptar`), o `status` e `schematize apps`.
//!
//! ## O bug que isto fecha
//!
//! `schematize overdev …` sem o `schematize-overdev` mandava rodar `schematize-market install
//! schematize-overdev`. O market não gere o overdev, e o repositório é privado: o comando
//! sugerido falhava. Mensagem que manda fazer o que não funciona é §37.48.
//!
//! Os dois fatos são DADOS aqui (nada de rede em tempo de execução) e o teste
//! `mercado_confere_com_o_appsdacasa_do_market` confere o primeiro contra o market.

/// O que se sabe de um app da casa para decidir como instalá-lo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origem {
    /// Nome do binário.
    pub bin: &'static str,
    /// Slug `dono/repositório` no GitHub.
    pub repo: &'static str,
    /// O `schematize-market install <bin>` reconhece este app (está em `EXTERNOS` do market).
    pub mercado: bool,
    /// O repositório é público (HTTP 200 sem autenticação, conferido em 2026-10-10).
    pub publico: bool,
}

/// A tabela. **Ao publicar `schematize_skills_rs`/`schematize_overdev_rs`, vire `publico`.**
pub const ORIGENS: &[Origem] = &[
    Origem {
        bin: "schematize-deployer",
        repo: "schematizeme/schematize_deployer_rs",
        mercado: true,
        publico: true,
    },
    Origem {
        bin: "schematize-optimizer",
        repo: "schematizeme/schematize_optimizer_rs",
        mercado: true,
        publico: true,
    },
    Origem {
        bin: "schematize-database",
        repo: "schematizeme/schematize_database_rs",
        mercado: true,
        publico: true,
    },
    Origem {
        bin: "schematize-git",
        repo: "schematizeme/schematize_git_rs",
        mercado: true,
        publico: true,
    },
    Origem {
        bin: "schematize-skills",
        repo: "schematizeme/schematize_skills_rs",
        mercado: true,
        publico: false,
    },
    Origem {
        bin: "schematize-overdev",
        repo: "schematizeme/schematize_overdev_rs",
        mercado: false,
        publico: false,
    },
    Origem {
        bin: "schematize-market",
        repo: "schematizeme/schematize_market_rs",
        mercado: false,
        publico: true,
    },
];

/// **O quê:** o texto de instalação do app. PURA — sem rede, só a tabela [`ORIGENS`].
///
/// **Onde:** [`crate::deployerlink::como_instalar_app`].
///
/// **Regra:** gerido pelo market E público -> `schematize-market install <app>`. Senão ->
/// `cargo install --git … --locked`, e, se o repo é privado, a nota honesta de que quem tem
/// acesso é quem consegue. App fora da tabela cai no comando do market (o comportamento de
/// sempre) em vez de inventar um repositório.
pub fn dica(bin: &str) -> String {
    let Some(o) = ORIGENS.iter().find(|o| o.bin == bin) else {
        return format!("schematize-market install {bin}");
    };
    if o.mercado && o.publico {
        return format!("schematize-market install {bin}");
    }
    let fonte = format!("cargo install --git https://github.com/{} --locked", o.repo);
    if o.publico {
        fonte
    } else {
        format!(
            "{fonte}\n    (o repositório ainda não é público; quem tem acesso instala com o \
             comando acima)"
        )
    }
}

/// **O quê:** os `bin: "…"` dentro de `EXTERNOS` num fonte do market. **Onde:** teste de guarda.
#[cfg(test)]
fn bins_do_externos(fonte: &str) -> Vec<String> {
    let Some(ini) = fonte.find("EXTERNOS") else { return vec![] };
    let resto = &fonte[ini..];
    let Some(fim) = resto.find("\n];") else { return vec![] };
    resto[..fim]
        .lines()
        .filter_map(|l| l.trim().strip_prefix("bin: \""))
        .filter_map(|l| l.split('"').next())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origem(bin: &str) -> &'static Origem {
        ORIGENS.iter().find(|o| o.bin == bin).unwrap_or_else(|| panic!("{bin} fora da tabela"))
    }

    /// O bug: overdev não é do market e o repo é privado — a dica não pode ser o comando do
    /// market, tem de ser o do fonte, com a nota honesta.
    #[test]
    fn overdev_nao_manda_para_o_market() {
        let d = dica("schematize-overdev");
        assert!(!d.contains("schematize-market"), "{d}");
        assert!(
            d.contains(
                "cargo install --git https://github.com/schematizeme/schematize_overdev_rs --locked"
            ),
            "{d}"
        );
        assert!(d.contains("ainda não é público"), "{d}");
    }

    /// Gerido e público: o comando do market, sem nota.
    #[test]
    fn app_gerido_e_publico_usa_o_market() {
        assert_eq!(dica("schematize-git"), "schematize-market install schematize-git");
        assert!(!dica("schematize-git").contains("público"));
    }

    /// Gerido mas privado (skills): o market tentaria clonar um 404. Vai o fonte + nota.
    #[test]
    fn app_gerido_mas_privado_nao_usa_o_market() {
        let d = dica("schematize-skills");
        assert!(!d.contains("schematize-market"), "{d}");
        assert!(d.contains("schematize_skills_rs") && d.contains("ainda não é público"), "{d}");
    }

    /// Todo app da tabela tem repo no padrão e uma dica não vazia; a dica nunca culpa.
    #[test]
    fn toda_dica_e_acionavel_e_sem_culpa() {
        assert_eq!(ORIGENS.len(), 7);
        for o in ORIGENS {
            assert!(o.repo.starts_with("schematizeme/schematize_"), "{}", o.bin);
            let d = dica(o.bin);
            assert!(d.contains("install"), "{d}");
            assert!(!d.contains("você errou"), "{d}");
        }
    }

    /// Cobre exatamente os apps que o hub conhece (`EXTERNOS`): app novo sem origem é erro.
    #[test]
    fn a_tabela_cobre_os_externos_do_hub() {
        for a in crate::deployerlink::EXTERNOS {
            origem(a.bin);
        }
        assert_eq!(ORIGENS.len(), crate::deployerlink::EXTERNOS.len());
    }

    /// **Guarda entre repos:** `mercado` tem de bater com a tabela `EXTERNOS` do market. Se o
    /// repo irmão não está ao lado (CI clona só este), PULA dizendo por quê — nunca passa mudo.
    #[test]
    fn mercado_confere_com_o_appsdacasa_do_market() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../schematize_market_rs/src/appsdacasa.rs");
        let Ok(fonte) = std::fs::read_to_string(&p) else {
            eprintln!(
                "PULADO: {} não existe (CI clona só este repo); a conferência roda local",
                p.display()
            );
            return;
        };
        let geridos = bins_do_externos(&fonte);
        assert!(!geridos.is_empty(), "não achei a tabela EXTERNOS em {}", p.display());
        for o in ORIGENS {
            assert_eq!(
                o.mercado,
                geridos.iter().any(|b| b == o.bin),
                "`{}`: `mercado` diverge da tabela do market",
                o.bin
            );
        }
    }

    /// O extrator acha só os `bin:` dentro do `EXTERNOS` — e falha alto se o formato mudar.
    #[test]
    fn extrator_le_so_o_bloco_externos() {
        let f = "const OUTRA: &[X] = &[ X { bin: \"nao\" } ];\n\
                 pub const EXTERNOS: &[AppExterno] = &[\n AppExterno {\n bin: \"a\",\n },\n];\n\
                 fn f() { let bin: \"nao2\"; }";
        assert_eq!(bins_do_externos(f), ["a"]);
    }
}
