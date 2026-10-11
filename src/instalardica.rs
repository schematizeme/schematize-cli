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
///
/// **`mercado` = "o market PUBLICADO (última tag) gere este app"** — não "o market de `main`".
/// O `schematize-overdev` já está em `EXTERNOS` no market de `main` (commit `bb6d47e`), mas fica
/// `mercado: false` até existir a tag `v0.4.1` que o inclui; o porquê está no teste
/// `mercado_confere_com_o_appsdacasa_do_market` (`SO_NO_MARKET_DE_MAIN`) e em `scripts/release-delegados.py`.
///
/// **Duas viradas destravam o corte de release do hub** (`sh scripts/verificar.sh --release`):
/// (1) `mercado: true` — SÓ DEPOIS de a tag do market existir e o `MARKET_MIN` do `install.sh`
/// subir para ela; (2) `publico: true` — SÓ DEPOIS de o dono publicar o repo (HTTP 200 sem
/// autenticação). Até lá o hub oferece o `cargo install --git` com a nota honesta.
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
        mercado: true,
        publico: true,
    },
    Origem {
        bin: "schematize-market",
        repo: "schematizeme/schematize_market_rs",
        mercado: false,
        publico: true,
    },
];

/// Apps para os quais o hub DELEGA um comando que um hook do Claude Code chama (`overdev check`
/// e `overdev guard`: Stop/PreToolUse). Se um deles falta, o hub sai rc=1 — e o Claude Code
/// trata isso como erro de hook NÃO bloqueante: o veto simplesmente some.
///
/// **Onde:** `scripts/release-delegados.py` (lê esta constante e a tabela [`ORIGENS`]); é o
/// guard que impede cortar release enquanto algum destes não for instalável numa máquina
/// limpa. O teste `criticos_de_hook_estao_na_tabela` ata a lista ao `overdevlink`.
pub const CRITICOS_DE_HOOK: &[&str] = &["schematize-overdev"];

/// **O quê:** `true` se o `schematize-market install <bin>` resolve este app (o market o gere E
/// o repo é público). PURA. App fora da tabela segue o comportamento de sempre (market).
///
/// **Onde:** `schematize apps install` — decide entre executar o market e só imprimir a [`dica`].
pub fn pelo_market(bin: &str) -> bool {
    ORIGENS.iter().find(|o| o.bin == bin).is_none_or(|o| o.mercado && o.publico)
}

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

    /// O overdev agora é gerido pelo market PUBLICADO (>= 0.4.2) e o repo é público: a dica é o
    /// comando do market, sem nota de "ainda não é público".
    #[test]
    fn overdev_vai_pelo_market() {
        assert_eq!(dica("schematize-overdev"), "schematize-market install schematize-overdev");
        assert!(!dica("schematize-overdev").contains("ainda não é público"));
    }

    /// Todo crítico de hook está na tabela, e o overdev (o delegado dos hooks) é crítico.
    #[test]
    fn criticos_de_hook_estao_na_tabela() {
        for b in CRITICOS_DE_HOOK {
            origem(b);
        }
        assert!(CRITICOS_DE_HOOK.contains(&crate::overdevlink::BIN));
    }

    /// `pelo_market` concorda com `dica`: market só quando gerido E público.
    #[test]
    fn pelo_market_so_quando_gerido_e_publico() {
        assert!(pelo_market("schematize-git"));
        assert!(pelo_market("schematize-overdev"));
        assert!(!pelo_market("schematize-skills"));
        for o in ORIGENS {
            assert_eq!(
                pelo_market(o.bin),
                dica(o.bin).starts_with("schematize-market"),
                "{}",
                o.bin
            );
        }
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

    /// Apps que o market de `main` já gere mas que o market PUBLICADO ainda não conhece.
    ///
    /// **Esta lista é o checklist da virada.** Cada bin aqui tem `mercado: false` na [`ORIGENS`]
    /// de propósito. Quando a release do market que o inclui existir (tag `v0.4.1`+, e o
    /// `MARKET_MIN` do `install.sh` subir junto), vire `mercado: true` na tabela E tire o bin
    /// daqui — o teste abaixo reprova se um dos dois ficar para trás.
    const SO_NO_MARKET_DE_MAIN: &[&str] = &[];

    /// **Guarda entre repos, com a semântica explícita.** `mercado` = "o market PUBLICADO gere
    /// este app" (é o que decide se a dica manda rodar o comando dele numa máquina de usuário).
    /// A tabela `EXTERNOS` do market em `main` = "o market de `main` gere". Logo:
    ///
    /// * `mercado: true` EXIGE o app em `EXTERNOS` — o publicado nunca sabe mais que `main`
    ///   (publicado ⊆ main). É o furo perigoso: dica que manda um comando que não existe.
    /// * App em `EXTERNOS` com `mercado: false` é PERMITIDO (main ⊇ publicado), mas só se estiver
    ///   em [`SO_NO_MARKET_DE_MAIN`] — a exceção é nomeada, não implícita.
    /// * E o inverso: bin em [`SO_NO_MARKET_DE_MAIN`] que já virou `mercado: true`, ou que o
    ///   market deixou de gerir, reprova — a lista não apodrece.
    ///
    /// Se o repo irmão não está ao lado (CI clona só este), PULA dizendo por quê — nunca passa mudo.
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
        let em_main = |bin: &str| geridos.iter().any(|b| b == bin);
        for o in ORIGENS {
            if o.mercado {
                assert!(
                    em_main(o.bin),
                    "`{}`: `mercado: true` mas o market de main não o gere — o publicado não \
                     pode saber mais que main",
                    o.bin
                );
                assert!(
                    !SO_NO_MARKET_DE_MAIN.contains(&o.bin),
                    "`{}`: já é `mercado: true` — tire-o de SO_NO_MARKET_DE_MAIN",
                    o.bin
                );
            } else if em_main(o.bin) {
                assert!(
                    SO_NO_MARKET_DE_MAIN.contains(&o.bin),
                    "`{}`: o market de main o gere e `mercado: false` não está justificado em \
                     SO_NO_MARKET_DE_MAIN",
                    o.bin
                );
            }
        }
        for b in SO_NO_MARKET_DE_MAIN {
            assert!(
                em_main(b),
                "`{b}` está em SO_NO_MARKET_DE_MAIN mas o market de main não o gere"
            );
            assert!(!origem(b).mercado, "`{b}` já é `mercado: true`");
        }
    }

    /// **O `install.sh` instala o overdev PELO market e, se falhar, diz a dica DESTA tabela.**
    /// Texto e tabela são dois lugares para o mesmo fato: sem este teste o hint do script
    /// envelhece quando o repo virar público (a dica passa a ser o comando do market).
    #[test]
    fn install_sh_instala_o_overdev_pelo_market_e_repete_a_dica() {
        let sh = include_str!("../install.sh");
        assert!(
            sh.contains("delega_ao_market schematize-overdev"),
            "o install.sh não pede o schematize-overdev ao market"
        );
        for linha in dica("schematize-overdev").lines() {
            assert!(
                sh.contains(linha.trim()),
                "a dica do overdev mudou em instalardica.rs e o install.sh não a acompanhou: \
                 falta `{}`",
                linha.trim()
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
