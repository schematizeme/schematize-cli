//! Encaminhamento para os apps da casa — o hub delega em vez de ter cópia.
//!
//! **O quê:** `ssh|vps|mcp` vão para o `schematize-deployer`; `env` para o `schematize-market`;
//! `db` para o `schematize-database`; `git`/`git-log` para o `schematize-git`; `skills` para o
//! `schematize-skills`; `disco` para o `schematize-optimizer`. Os argumentos são repassados **crus**.
//!
//! **Onde:** interceptado no `main`, ANTES do `clap`.
//!
//! ## O que isto conserta, e por que doeu antes
//!
//! Os ADR-0010 e 0012 disseram que `sshkeys`, `vps`, `mcp` e `environments` sairiam do hub.
//! Nenhuma das extrações moveu: **todas copiaram**. Ficaram 9.827 linhas duplicadas, que já
//! divergiram — e o custo apareceu duas vezes no mesmo dia:
//!
//! - `schematize env install csharp --method distro` recusou instalar o .NET **depois** de o
//!   market já saber adicionar o repo do fornecedor;
//! - `schematize ssh import` seguiu exigindo arquivo **depois** de o deployer aceitar colagem.
//!
//! Nos dois casos o conserto existia, na outra cópia. Verificar que o repo NOVO tem o código
//! não é o mesmo que verificar que o ANTIGO deixou de ter — e no `git log` as duas coisas se
//! parecem.
//!
//! ## Por que ANTES do `clap`, e não um braço do despacho
//!
//! Se o hub parseasse, ele precisaria conhecer cada flag dos apps — e `schematize ssh import
//! --paste` morreria em "unexpected argument" porque o `--paste` nasceu no deployer. Passando
//! cru, **o hub nunca mais precisa saber** o que os apps aceitam: quem valida é o dono.
//!
//! É o oposto do que criou o problema. Toda flag nova nos apps já chega pelo hub, no dia em
//! que nasce, sem ninguém sincronizar nada.

/// Um domínio que o hub não resolve mais: para qual app vai, e se o próprio nome do domínio
/// viaja junto.
///
/// **`mantem_prefixo` não é detalhe.** O deployer conservou os subcomandos (`schematize ssh
/// list` vira `schematize-deployer ssh list`), mas o market promoveu os dele a comandos de
/// topo — lá `env install` vira `install`, porque `schematize-market env` não existe. Sem esta
/// distinção o encaminhamento morre em "unrecognized subcommand", que é o que aconteceu na
/// primeira tentativa.
struct Delegado {
    /// O subcomando do hub que deixa de ser dele.
    prefixo: &'static str,
    /// O binário que passa a resolver.
    bin: &'static str,
    /// O `prefixo` viaja para o app (deployer) ou é consumido aqui (market)?
    mantem_prefixo: bool,
    /// O token que SUBSTITUI o prefixo no app dono, quando o nome muda de forma.
    ///
    /// **Existe por causa do `git-log`.** Ele era um comando de TOPO deste hub, e no app dono a
    /// mesma coisa é `schematize-git log`. Sem isto restariam duas saídas ruins: consumir o
    /// prefixo (e mandar as flags sem subcomando nenhum) ou mantê-lo (e mandar um `git-log`
    /// que o app não conhece). As duas quebram um comando que alguém já tem no dedo.
    ///
    /// `None` é o caso normal: o prefixo viaja (deployer) ou some (market, database, git).
    vira: Option<&'static str>,
    /// A decisão que tirou este domínio daqui — vai na mensagem de "não instalado".
    ///
    /// **É campo, e não literal na mensagem, porque o literal MENTIU.** A frase cravava
    /// "ADR-0010/0012" para todo delegado, e o `db` saiu pelo ADR-0018. Quem fosse checar a
    /// decisão leria uma que não fala do banco, e concluiria que a mensagem está errada sobre
    /// o resto também.
    adr: &'static str,
}

/// Os domínios delegados. **Fonte única:** acrescentar um aqui é a única mudança necessária.
const DELEGADOS: &[Delegado] = &[
    Delegado {
        prefixo: "ssh",
        bin: crate::deployerlink::BIN,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0010",
    },
    Delegado {
        prefixo: "vps",
        bin: crate::deployerlink::BIN,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0010",
    },
    Delegado {
        prefixo: "mcp",
        bin: crate::deployerlink::BIN,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0010",
    },
    Delegado {
        prefixo: "env",
        bin: crate::deployerlink::GESTOR,
        mantem_prefixo: false,
        vira: None,
        adr: "ADR-0012",
    },
    // `db` não mantém o prefixo, como o `env`: no app dono os subcomandos são de TOPO
    // (`schematize-database introspect`), e não `schematize-database db introspect`. Mandar o
    // prefixo junto morreria em "unrecognized subcommand", que foi o que aconteceu na primeira
    // tentativa de delegar o market.
    Delegado {
        prefixo: "db",
        bin: crate::deployerlink::DATABASE,
        mantem_prefixo: false,
        vira: None,
        adr: "ADR-0018",
    },
    Delegado {
        prefixo: "git",
        bin: crate::deployerlink::GIT,
        mantem_prefixo: false,
        vira: None,
        adr: "ADR-0019",
    },
    // `git-log` era comando de TOPO daqui; no app dono é `schematize-git log`.
    Delegado {
        prefixo: "skills",
        bin: crate::deployerlink::SKILLS,
        mantem_prefixo: false,
        vira: None,
        adr: "ADR-0012 F4",
    },
    // Os quatro aliases OCULTOS que existiam no topo desta CLI antes de `skills <sub>`.
    // Mantêm o prefixo: lá eles são o próprio verbo.
    Delegado {
        prefixo: "install",
        bin: crate::deployerlink::SKILLS,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F4",
    },
    Delegado {
        prefixo: "update",
        bin: crate::deployerlink::SKILLS,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F4",
    },
    Delegado {
        prefixo: "list",
        bin: crate::deployerlink::SKILLS,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F4",
    },
    Delegado {
        prefixo: "remove",
        bin: crate::deployerlink::SKILLS,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F4",
    },
    // **O overdev, e com ele os HOOKS do Claude Code.**
    //
    // O `PreToolUse` roda `schematize overdev guard` a cada chamada de ferramenta e o `Stop`
    // roda `schematize overdev check` a cada parada. Esta linha é o que mantém as duas coisas
    // funcionando depois que o domínio saiu — e é por isso que ela entrou no repo ANTES de o
    // domínio sair, e não junto: uma janela em que o binário instalado não sabe responder
    // `overdev guard` bloqueia toda sessão aberta.
    Delegado {
        prefixo: "overdev",
        bin: crate::deployerlink::OVERDEV,
        mantem_prefixo: false,
        vira: None,
        adr: "ADR-0012 F5",
    },
    // A caixa de entrada do overdev — `schematize caixa` e o atalho `schematize add`.
    Delegado {
        prefixo: "caixa",
        bin: crate::deployerlink::OVERDEV,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F5",
    },
    Delegado {
        prefixo: "add",
        bin: crate::deployerlink::OVERDEV,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F5",
    },
    // O `archive` sincroniza o `<projeto>_archive`, que é onde o overdev grava handoff,
    // contexto e índice. Foi com ele.
    Delegado {
        prefixo: "archive",
        bin: crate::deployerlink::OVERDEV,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F5",
    },
    // O `graph obsidian` exporta o ÍNDICE como vault do Obsidian, e o exportador vive no
    // `panel/` — que foi com o overdev. O índice é do hub, mas o parser do grafo é o mesmo que
    // o painel HTML usa: ter dois leitores do mesmo formato é o que esta extradição desfaz.
    Delegado {
        prefixo: "graph",
        bin: crate::deployerlink::OVERDEV,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F5",
    },
    Delegado {
        prefixo: "panel",
        bin: crate::deployerlink::OVERDEV,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F5",
    },
    // O governador de concorrência: ele existe para o SPLIT do overdev saber quantos agentes
    // cabem antes de lançar.
    Delegado {
        prefixo: "agents",
        bin: crate::deployerlink::OVERDEV,
        mantem_prefixo: true,
        vira: None,
        adr: "ADR-0012 F5",
    },
    // `disco` foi para o optimizer na E3, mas o hub manteve uma COPIA inteira até o D9. Aqui
    // o prefixo é consumido E os subcomandos mudam de forma (`list|clean|docker` viram
    // `disco|disco-clean|disco-docker` no app dono) — ver [`traduz_disco`].
    Delegado {
        prefixo: "disco",
        bin: crate::deployerlink::OPTIMIZER,
        mantem_prefixo: false,
        vira: None,
        adr: "ADR-0018 E3",
    },
    Delegado {
        prefixo: "git-log",
        bin: crate::deployerlink::GIT,
        mantem_prefixo: false,
        vira: Some("log"),
        adr: "ADR-0019",
    },
];

/// **O quê:** para qual app um `argv` vai, e com quais argumentos. PURA.
///
/// **Onde:** [`interceptar`]. Separada da execução porque é a REGRA — e a regra tem casos
/// (argv vazio, prefixo parcial, prefixo consumido) que só um teste cobre bem.
pub fn destino(argv: &[String]) -> Option<(&'static str, Vec<String>)> {
    delegado(argv).map(|(d, args)| (d.bin, args))
}

/// **O quê:** os argumentos de `schematize disco <sub> …` na forma do `schematize-optimizer`.
/// PURA.
///
/// **Onde:** [`delegado`], só para o prefixo `disco`.
///
/// **O mapa:** `list [--min-dias N]` -> `disco [--min-dias N]`; `clean …` -> `disco-clean …`;
/// `docker …` -> `disco-docker …`. Flags passam cruas (`--min-dias`, `--tipo`, `--montagem`,
/// `-y/--yes`, `--podar` têm o mesmo nome nos dois). Sem subcomando, ou só `--help`, o app dono
/// responde — o hub não valida flag de ninguém.
fn traduz_disco(resto: &[String]) -> Vec<String> {
    let (verbo, cauda) = match resto.split_first() {
        Some((v, c)) => match v.as_str() {
            "list" => ("disco", c),
            "clean" => ("disco-clean", c),
            "docker" => ("disco-docker", c),
            _ => return resto.to_vec(),
        },
        None => return vec!["disco".to_string()],
    };
    let mut out = vec![verbo.to_string()];
    out.extend(cauda.iter().cloned());
    out
}

/// **O quê:** o delegado inteiro e os argumentos dele. PURA.
///
/// **Onde:** [`destino`] e [`interceptar`] — este precisa do `adr` para a mensagem de ausência,
/// e aquele só do binário.
fn delegado(argv: &[String]) -> Option<(&'static Delegado, Vec<String>)> {
    let primeiro = argv.first()?.as_str();
    let d = DELEGADOS.iter().find(|d| d.prefixo == primeiro)?;
    if d.prefixo == "disco" {
        return Some((d, traduz_disco(&argv[1..])));
    }
    let mut args = if d.mantem_prefixo { argv.to_vec() } else { argv[1..].to_vec() };
    if let Some(novo) = d.vira {
        args.insert(0, novo.to_string());
    }
    Some((d, args))
}

/// **O quê:** o `argv` é um `install`/`update` de skills (direto ou via `skills <sub>`)? PURA.
///
/// **Onde:** [`interceptar`], para decidir se aplica o núcleo sempre-on ao fim.
pub fn instala_ou_atualiza_skills(argv: &[String]) -> bool {
    let a = |i: usize| argv.get(i).map(String::as_str);
    matches!(a(0), Some("install" | "update"))
        || (a(0) == Some("skills") && matches!(a(1), Some("install" | "update")))
}

/// **O quê:** se o comando é de um app da casa, executa lá e devolve o código de saída.
/// `None` = não é delegado, o `clap` do hub segue normalmente.
///
/// **Onde:** a primeira linha do `main`.
///
/// **Herda o terminal de propósito:** `ssh run` abre sessão interativa, `import --paste` lê da
/// entrada padrão, e instalar pede sudo. Capturar a saída quebraria os três.
pub fn interceptar(argv: &[String]) -> Option<i32> {
    let (d, args) = delegado(argv)?;
    let bin = d.bin;
    let Some(caminho) = crate::binlink::resolve_bin(bin) else {
        // **Os HOOKS do overdev não podem sair rc=1**: o Claude Code trata isso como erro NÃO
        // bloqueante e o veto de um run ativo some em silêncio. `overdevausente` responde
        // rc=0 (com o JSON de bloqueio quando cabe). Ver o doc dele.
        if d.prefixo == "overdev" {
            if let Some(rc) = crate::overdevausente::executar(&args) {
                return Some(rc);
            }
        }
        // **O comando que se manda rodar tem de INSTALAR este app**, e o que estava aqui não
        // instalava: era o `curl | bash` do `install.sh`, que põe o `schematize` e o
        // `schematize-market` e mais ninguém (ADR-0013 — quem instala app da casa é o market).
        // Quem seguisse a instrução rodaria um instalador longo e tentaria de novo, com o mesmo
        // erro, sem nada dizendo por quê. Mensagem que não resolve é §37.48.
        eprintln!(
            "erro: `{bin}` não está instalado — é ele que cuida de `{}` desde o {}.\n\
             Instale com:\n    {}",
            argv[0],
            d.adr,
            crate::deployerlink::como_instalar_app(bin)
        );
        return Some(1);
    };
    match std::process::Command::new(&caminho).args(&args).status() {
        // 128+sinal é a convenção do shell para "morreu de sinal"; sem isto um Ctrl-C no app
        // delegado faria o hub sair 0, e um script encadeado seguiria como se tivesse dado certo.
        Ok(st) => {
            let code = st.code().unwrap_or(130);
            // Núcleo sempre-on: ao fim de um install/update de skills bem-sucedido, aplica o
            // CLAUDE.md global + hook SessionStart (best-effort; só age se a engineering está lá).
            if code == 0 && instala_ou_atualiza_skills(argv) {
                crate::nucleo::garantir_apos_install();
            }
            Some(code)
        }
        Err(e) => {
            eprintln!("erro: não consegui executar `{}`: {e}", caminho.display());
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os quatro domínios delegados vão para o app certo — `env` para o gestor, o resto para o
    /// deployer. Trocar um pelo outro daria "subcomando desconhecido" num app que não é o dono.
    #[test]
    fn cada_dominio_vai_para_o_seu_app() {
        let a = |s: &str| vec![s.to_string()];
        for c in ["ssh", "vps", "mcp"] {
            let (bin, args) = destino(&a(c)).expect(c);
            assert_eq!(bin, crate::deployerlink::BIN);
            // O deployer conservou os subcomandos: o prefixo VIAJA.
            assert_eq!(args, [c], "o deployer precisa receber `{c}`");
        }
        let (bin, args) = destino(&a("env")).expect("env");
        assert_eq!(bin, crate::deployerlink::GESTOR);
        // O market promoveu os comandos a topo: o prefixo é CONSUMIDO aqui.
        assert!(args.is_empty(), "`schematize-market env` não existe: {args:?}");
    }

    /// O caso que quebrou na primeira tentativa: `env install csharp --method distro` tem de
    /// virar `install csharp --method distro`, sem o `env`. Com ele, o market responde
    /// "unrecognized subcommand" e a pessoa leva um erro do app errado.
    #[test]
    fn env_perde_o_prefixo_e_o_resto_dos_argumentos_fica() {
        let argv: Vec<String> = ["env", "install", "csharp", "--method", "distro"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (_, args) = destino(&argv).unwrap();
        assert_eq!(args, ["install", "csharp", "--method", "distro"]);
    }

    /// E o ssh NÃO perde: `ssh import --paste` chega inteiro no deployer.
    #[test]
    fn ssh_chega_inteiro_no_deployer() {
        let argv: Vec<String> =
            ["ssh", "import", "--paste", "--name", "k"].iter().map(|s| s.to_string()).collect();
        let (_, args) = destino(&argv).unwrap();
        assert_eq!(args, ["ssh", "import", "--paste", "--name", "k"]);
    }

    /// **A forma exata dos HOOKS do Claude Code:** `overdev check|guard` chegam ao
    /// `schematize-overdev` como `check`/`guard` — sem o prefixo, que o app não conhece.
    #[test]
    fn os_hooks_do_overdev_chegam_como_check_e_guard() {
        for sub in ["check", "guard"] {
            let argv: Vec<String> = ["overdev", sub].iter().map(|s| s.to_string()).collect();
            let (bin, args) = destino(&argv).expect(sub);
            assert_eq!(bin, crate::deployerlink::OVERDEV);
            assert_eq!(args, [sub]);
        }
    }

    /// O que NÃO é delegado passa direto para o `clap` do hub — interceptar demais roubaria
    /// comandos que são do hub.
    #[test]
    fn o_que_e_do_hub_nao_e_interceptado() {
        // **Esta lista ENCOLHE a cada fase, e é assim que se sabe que a extradição andou.**
        // `skills` saiu na E5; `overdev` na E6. Um teste que guarda "o que é do hub" tem de
        // acompanhar — se não acompanhasse, teria reprovado sobre um corte correto, e a saída
        // fácil seria apagar o teste em vez de o dado.
        //
        // O que sobrou aqui é o hub de verdade: diagnosticar, mostrar estado, abrir a janela,
        // e o registro de projetos que o seletor da janela principal lê.
        for c in ["status", "doctor", "gui", "--help", "-V", "projects"] {
            assert!(destino(&[c.to_string()]).is_none(), "`{c}` não devia ser delegado");
        }
        assert!(destino(&[]).is_none());
        // E o que É delegado tem de estar delegado: a outra metade da mesma afirmação.
        for (c, bin) in [
            ("skills", crate::deployerlink::SKILLS),
            ("install", crate::deployerlink::SKILLS),
            ("db", crate::deployerlink::DATABASE),
            ("git", crate::deployerlink::GIT),
            ("ssh", crate::deployerlink::BIN),
            ("env", crate::deployerlink::GESTOR),
            ("disco", crate::deployerlink::OPTIMIZER),
            ("overdev", crate::deployerlink::OVERDEV),
            ("agents", crate::deployerlink::OVERDEV),
            ("archive", crate::deployerlink::OVERDEV),
            ("caixa", crate::deployerlink::OVERDEV),
        ] {
            let (dest, _) = destino(&[c.to_string()]).unwrap_or_else(|| panic!("`{c}` não delega"));
            assert_eq!(dest, bin, "`{c}` foi para o app errado");
        }
    }

    /// `disco` vai para o optimizer com os subcomandos traduzidos — e flags passam cruas.
    #[test]
    fn disco_vira_os_verbos_do_optimizer() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let casos: [(&[&str], &[&str]); 7] = [
            (&["disco"], &["disco"]),
            (&["disco", "list"], &["disco"]),
            (&["disco", "list", "--min-dias", "5"], &["disco", "--min-dias", "5"]),
            (
                &["disco", "clean", "--tipo", "target", "-y"],
                &["disco-clean", "--tipo", "target", "-y"],
            ),
            (&["disco", "docker", "--podar", "x"], &["disco-docker", "--podar", "x"]),
            (&["disco", "docker"], &["disco-docker"]),
            (&["disco", "--help"], &["--help"]),
        ];
        for (entrada, saida) in casos {
            let (bin, args) = destino(&v(entrada)).expect("disco delega");
            assert_eq!(bin, crate::deployerlink::OPTIMIZER);
            assert_eq!(args, v(saida), "{entrada:?}");
        }
    }

    /// `install`/`update` de skills disparam o núcleo; o resto não.
    #[test]
    fn so_install_e_update_de_skills_aplicam_o_nucleo() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(instala_ou_atualiza_skills(&v(&["install", "--all"])));
        assert!(instala_ou_atualiza_skills(&v(&["update"])));
        assert!(instala_ou_atualiza_skills(&v(&["skills", "install", "rust"])));
        assert!(instala_ou_atualiza_skills(&v(&["skills", "update"])));
        assert!(!instala_ou_atualiza_skills(&v(&["skills", "list"])));
        assert!(!instala_ou_atualiza_skills(&v(&["remove", "x"])));
        assert!(!instala_ou_atualiza_skills(&v(&["ssh", "install"])));
        assert!(!instala_ou_atualiza_skills(&[]));
    }

    /// Prefixo PARCIAL não conta: `sshfoo` não é `ssh`. Sem a igualdade exata, um comando
    /// futuro do hub começando com as mesmas letras seria sequestrado.
    #[test]
    fn casa_o_subcomando_inteiro_e_nao_o_prefixo() {
        assert!(destino(&["sshfoo".to_string()]).is_none());
        assert!(destino(&["environments".to_string()]).is_none());
    }
}
