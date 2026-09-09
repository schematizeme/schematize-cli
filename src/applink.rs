//! Encaminhamento para os apps da casa — o hub delega em vez de ter cópia.
//!
//! **O quê:** `schematize ssh|vps|mcp …` vai para o `schematize-deployer`; `schematize env …`
//! vai para o `schematize-market`. Os argumentos são repassados **crus**.
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
}

/// Os domínios delegados. **Fonte única:** acrescentar um aqui é a única mudança necessária.
const DELEGADOS: &[Delegado] = &[
    Delegado { prefixo: "ssh", bin: crate::deployerlink::BIN, mantem_prefixo: true },
    Delegado { prefixo: "vps", bin: crate::deployerlink::BIN, mantem_prefixo: true },
    Delegado { prefixo: "mcp", bin: crate::deployerlink::BIN, mantem_prefixo: true },
    Delegado { prefixo: "env", bin: crate::deployerlink::GESTOR, mantem_prefixo: false },
];

/// **O quê:** para qual app um `argv` vai, e com quais argumentos. PURA.
///
/// **Onde:** [`interceptar`]. Separada da execução porque é a REGRA — e a regra tem casos
/// (argv vazio, prefixo parcial, prefixo consumido) que só um teste cobre bem.
pub fn destino(argv: &[String]) -> Option<(&'static str, Vec<String>)> {
    let primeiro = argv.first()?.as_str();
    let d = DELEGADOS.iter().find(|d| d.prefixo == primeiro)?;
    let args = if d.mantem_prefixo { argv.to_vec() } else { argv[1..].to_vec() };
    Some((d.bin, args))
}

/// **O quê:** se o comando é de um app da casa, executa lá e devolve o código de saída.
/// `None` = não é delegado, o `clap` do hub segue normalmente.
///
/// **Onde:** a primeira linha do `main`.
///
/// **Herda o terminal de propósito:** `ssh run` abre sessão interativa, `import --paste` lê da
/// entrada padrão, e instalar pede sudo. Capturar a saída quebraria os três.
pub fn interceptar(argv: &[String]) -> Option<i32> {
    let (bin, args) = destino(argv)?;
    let Some(caminho) = crate::agentrun::resolve_bin(bin) else {
        eprintln!(
            "erro: `{bin}` não está instalado — é ele que cuida de `{}` desde o ADR-0010/0012.\n\
             Instale com:\n    curl -fsSL {} | bash",
            argv[0],
            crate::upgrade::INSTALL_SH
        );
        return Some(1);
    };
    match std::process::Command::new(&caminho).args(&args).status() {
        // 128+sinal é a convenção do shell para "morreu de sinal"; sem isto um Ctrl-C no app
        // delegado faria o hub sair 0, e um script encadeado seguiria como se tivesse dado certo.
        Ok(st) => Some(st.code().unwrap_or(130)),
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

    /// O que NÃO é delegado passa direto para o `clap` do hub — interceptar demais roubaria
    /// comandos que são do hub.
    #[test]
    fn o_que_e_do_hub_nao_e_interceptado() {
        for c in ["skills", "overdev", "status", "doctor", "gui", "--help", "-V", "projects"] {
            assert!(destino(&[c.to_string()]).is_none(), "`{c}` não devia ser delegado");
        }
        assert!(destino(&[]).is_none());
    }

    /// Prefixo PARCIAL não conta: `sshfoo` não é `ssh`. Sem a igualdade exata, um comando
    /// futuro do hub começando com as mesmas letras seria sequestrado.
    #[test]
    fn casa_o_subcomando_inteiro_e_nao_o_prefixo() {
        assert!(destino(&["sshfoo".to_string()]).is_none());
        assert!(destino(&["environments".to_string()]).is_none());
    }
}
