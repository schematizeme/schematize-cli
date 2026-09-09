//! PONTE com o **schematize deployer** — descoberta e chamada, nunca acoplamento.
//!
//! **O quê:** descobre se o Deployer está instalado, qual versão, e o invoca como
//! subprocesso quando o usuário pede.
//!
//! **Onde:** `schematize deployer <sub>` (CLI) e, adiante, o card da GUI.
//!
//! ## As três regras desta ponte
//!
//! **1. A ausência do Deployer nunca derruba o schematize** (piso 10). Todo caminho aqui
//! devolve "não instalado" como *estado*, não como erro. O schematize funcionava sem ele
//! ontem e continua funcionando hoje — a ponte é conveniência, não dependência.
//!
//! **2. Subprocesso, não biblioteca.** Ligar os dois crates faria o schematize compilar o
//! Deployer junto, e aí não haveria dois apps: haveria um monólito com dois nomes. É o que o
//! ADR-0010 decidiu evitar.
//!
//! **3. O Deployer nunca devolve segredo por aqui — e é o ponto.** Esta ponte lê **versão** e
//! **código de saída**. Se um dia alguém quiser trazer a passphrase do cofre ou o conteúdo de
//! uma chave por este canal, a resposta é não: o motivo de o Deployer existir é justamente
//! que o segredo **não** transite pelo processo onde o agente opera.

use crate::agentrun::resolve_bin;
use std::path::PathBuf;

/// Um app EXTERNO do ecossistema — instalável à parte, e que o schematize apenas conhece.
///
/// **Por que uma tabela e não um módulo por app:** a lógica de descobrir, versionar e
/// instalar é idêntica; duplicá-la por app seria a divergência esperando acontecer. O que
/// muda entre eles é só o nome do binário e a frase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppExterno {
    /// Nome do binário no `$PATH`.
    pub bin: &'static str,
    /// Uma linha sobre o que ele faz — vai no `status` e na aba do Mercado.
    pub sobre: &'static str,
}

// O campo `flag` (a flag do `install.sh`) SAIU no ADR-0013: quem instala app da casa é o
// `schematize-market`, e a flag era só o argumento do `curl | bash` que deixou de existir.

/// Os apps externos que o schematize conhece.
pub const EXTERNOS: &[AppExterno] = &[
    AppExterno {
        bin: "schematize-deployer",
        sobre: "SSH, VPS e acesso remoto auditado, com a credencial no cofre",
    },
    AppExterno {
        bin: "schematize-optimizer",
        sobre: "mede o ambiente de dev e põe cada software no seu teto de recurso",
    },
    AppExterno {
        bin: "schematize-market",
        sobre: "instala e atualiza tudo do ecossistema — runtimes, ferramentas e os apps da casa",
    },
];

/// **O quê:** o nome que este binário TEVE antes de ser renomeado, se houve renomeação.
///
/// **Onde:** [`descobrir_app`], como segunda tentativa.
///
/// **Por que existe:** o Deployer se chamou `deployer` até o commit `0fa0112` do repo dele.
/// Numa máquina que o instalou antes disso, é esse o arquivo que está em `~/.cargo/bin` — e
/// procurar só o nome novo faz o `status` e a aba do Mercado na GUI afirmarem "não instalado"
/// sobre um app que está lá. Medido: `schematize-market list` dizia isso de um deployer 0.5.0.
///
/// **Por que a lista é curta e vive aqui:** o hub não depende do crate do market (ADR-0012), e
/// duplicar a tabela inteira seria pior. O que se duplica é UM par nome-novo → nome-velho, com
/// teste dos dois lados; a alternativa era o hub não saber nada e continuar mentindo.
fn nome_legado(bin: &str) -> Option<&'static str> {
    match bin {
        "schematize-deployer" => Some("deployer"),
        _ => None,
    }
}

/// **O quê:** acha um app externo pelo nome do binário.
/// **Onde:** a CLI, ao despachar `schematize <app> …`.
pub fn externo(bin: &str) -> Option<&'static AppExterno> {
    EXTERNOS.iter().find(|a| a.bin == bin)
}

/// Nome do binário do Deployer.
pub const BIN: &str = "schematize-deployer";
/// Repositório, para a mensagem de instalação e para o `install.sh`.
pub const REPO: &str = "schematizeme/schematize_deployer_rs";

/// O que se sabe do Deployer nesta máquina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Estado {
    /// Instalado, com o caminho e a versão que ele reporta.
    Instalado { caminho: PathBuf, versao: String },
    /// Não está no `$PATH` nem nos diretórios de fallback.
    Ausente,
    /// O binário existe mas não respondeu `--version` — instalação quebrada.
    ///
    /// É um terceiro estado de propósito: dizer "ausente" para um binário que ESTÁ lá mandaria
    /// a pessoa reinstalar o que já tem, e esconderia a causa real (permissão, biblioteca
    /// faltando, arquitetura errada).
    Quebrado { caminho: PathBuf, erro: String },
}

impl Estado {
    /// **O quê:** `true` só quando dá para usar o Deployer agora.
    /// **Onde:** a GUI e o `doctor`, para decidir entre "abrir" e "instalar".
    pub fn utilizavel(&self) -> bool {
        matches!(self, Estado::Instalado { .. })
    }
}

/// **O quê:** descobre o Deployer nesta máquina.
///
/// **Onde:** `schematize deployer status`, o `doctor` e a GUI.
///
/// Usa o mesmo `resolve_bin` do resto do app — que varre o `$PATH` **e** os diretórios de
/// fallback. Sem isso, o app aberto pelo lançador do desktop (que dá PATH mínimo) diria
/// "não instalado" sobre um Deployer que está em `~/.cargo/bin`.
pub fn descobrir_app(bin: &str) -> Estado {
    let Some(caminho) = resolve_bin(bin).or_else(|| nome_legado(bin).and_then(resolve_bin)) else {
        return Estado::Ausente;
    };
    match crate::util::run(&caminho.to_string_lossy(), &["--version"]) {
        Ok(saida) => {
            let versao = saida.split_whitespace().nth(1).unwrap_or("?").to_string();
            Estado::Instalado { caminho, versao }
        }
        Err(e) => Estado::Quebrado { caminho, erro: e },
    }
}

/// **O quê:** o estado do Deployer. **Onde:** compat com quem já chamava.
pub fn descobrir() -> Estado {
    descobrir_app(BIN)
}

/// **O quê:** a linha de comando que instala um app da casa nesta máquina.
///
/// **Onde:** o `status` da CLI e a aba do Mercado na GUI. Função PURA — devolve o texto,
/// não executa.
///
/// **Quem instala é o `schematize-market` (ADR-0013), e isso mudou aqui.** Este texto era
/// `curl -fsSL <install.sh> | bash -s -- --deployer`. Havia três lugares montando esse mesmo
/// `curl | bash` — este, o market e o próprio script —, cada um com um comportamento, e
/// nenhum deles dono. O market absorveu o updater e passou a ser o único responsável por
/// instalar e atualizar; delegar a ele é o que faz este módulo parar de ser a terceira cópia.
///
/// **Por que não instala sozinho:** instalar compila um app inteiro, pede rede e leva
/// minutos. Fazer isso como efeito colateral de um `status` seria surpresa cara. O comando
/// fica visível para a pessoa rodar quando quiser.
pub fn como_instalar_app(bin: &str) -> String {
    format!("{GESTOR} install {bin}")
}

/// O nome do gestor. Constante porque ele aparece em texto E em execução, e escrever a string
/// nos dois lugares é como o nome legado do deployer sobreviveu num deles.
pub const GESTOR: &str = "schematize-market";

/// **O quê:** o comando que EXECUTA a instalação, com o gestor resolvido em caminho absoluto.
///
/// **Onde:** `schematize apps install`, antes do `bash -c`.
///
/// ## Por que não dá pra executar o texto de [`como_instalar_app`]
///
/// Aquele texto é para a pessoa LER e copiar, e para isso o nome puro é o certo — num terminal
/// normal o `$PATH` tem `~/.cargo/bin`. Mas quem executa nem sempre tem esse `$PATH`: a janela
/// aberta pelo lançador do desktop recebe um PATH mínimo, e o terminal que ela abre herda.
///
/// Foi assim que um clique em "instalar" respondeu `schematize-market: comando não encontrado`
/// sobre um gestor instalado. A GUI foi corrigida; esta é a MESMA falha no caminho da CLI, que
/// também executa via `bash -c`. Corrigir só onde alguém clicou deixaria a outra metade
/// esperando o próximo clique — que é a forma como este bug já voltou uma vez.
///
/// Devolve `None` quando o gestor não está em lugar nenhum: aí o chamador diz o que falta, em
/// vez de deixar o `bash` reclamar de um nome.
pub fn comando_de_instalacao(bin: &str) -> Option<String> {
    comando_de_instalacao_com(GESTOR, bin)
}

/// **O quê:** [`comando_de_instalacao`] com o nome do gestor injetado — é o que permite testar
/// o caminho "gestor ausente" sem depender do que está instalado na máquina de quem roda.
pub fn comando_de_instalacao_com(gestor: &str, bin: &str) -> Option<String> {
    let caminho = resolve_bin(gestor)?;
    Some(format!("{} install {bin}", caminho.display()))
}

/// **O quê:** como instalar o Deployer. **Onde:** compat com quem já chamava.
pub fn como_instalar() -> String {
    como_instalar_app(BIN)
}

#[cfg(test)]
mod tests {

    /// **O buraco que isto fecha.** O comando que EXECUTA usava o nome puro do gestor, e a
    /// janela aberta pelo lançador do desktop tem PATH mínimo (sem `~/.cargo/bin`). O clique em
    /// "instalar" respondia `schematize-market: comando não encontrado` sobre um gestor
    /// instalado. Reproduzido com `env -i PATH=/usr/bin:/bin bash -c 'schematize-market -V'`.
    ///
    /// O texto MOSTRADO segue com o nome puro de propósito — é o que a pessoa copia num
    /// terminal normal. Quem tem de ser absoluto é o que roda.
    #[test]
    fn o_texto_e_para_ler_e_o_comando_e_para_executar() {
        let texto = como_instalar_app("schematize-deployer");
        assert_eq!(texto, "schematize-market install schematize-deployer");

        // O executável, quando o gestor existe, vem com caminho — nunca com o nome puro.
        if let Some(cmd) = comando_de_instalacao("schematize-deployer") {
            assert!(cmd.contains("/"), "o comando executado tem de ser absoluto: {cmd}");
            assert!(cmd.ends_with(" install schematize-deployer"), "{cmd}");
            assert!(!cmd.starts_with("schematize-market "), "voltou ao nome puro: {cmd}");
        }
    }

    /// Gestor ausente devolve `None` para o chamador dizer o que falta — em vez de mandar um
    /// nome para o `bash` reclamar. É a diferença entre "instale o gestor com X" e uma linha
    /// crua de shell.
    #[test]
    fn gestor_inexistente_nao_vira_comando() {
        assert_eq!(comando_de_instalacao_com("nao-existe-mesmo-xyz", "app"), None);
    }
    use super::*;

    /// `utilizavel` só é verdade no estado que de fato dá para usar. Sem esta distinção, um
    /// binário quebrado seria tratado como bom e a GUI abriria o que não abre.
    #[test]
    fn so_o_instalado_e_utilizavel() {
        let bom =
            Estado::Instalado { caminho: "/x/schematize-deployer".into(), versao: "0.2.1".into() };
        assert!(bom.utilizavel());
        assert!(!Estado::Ausente.utilizavel());
        let ruim =
            Estado::Quebrado { caminho: "/x/schematize-deployer".into(), erro: "libc".into() };
        assert!(!ruim.utilizavel(), "binário que não responde não pode contar como instalado");
    }

    /// A instrução de instalação DELEGA ao market (ADR-0013) — nada de `curl | bash`.
    ///
    /// Havia três lugares montando o mesmo `curl -fsSL <install.sh> | bash`: este, o market
    /// e o próprio script. Se algum voltar a montá-lo, volta a duplicação que o ADR-0013
    /// existe para acabar — e, no caso do market, um LOOP (o script delega a ele).
    #[test]
    fn a_instrucao_de_instalar_delega_ao_market() {
        let c = como_instalar();
        assert!(c.starts_with("schematize-market install"), "tem de delegar ao gestor: {c}");
        assert!(c.contains(BIN), "e nomear o app certo: {c}");
        for proibido in ["curl", "install.sh", "bash -s", "--deployer"] {
            assert!(!c.contains(proibido), "`{proibido}` voltou ao caminho de instalar: {c}");
        }
        // E vale para todo app da tabela, não só o deployer.
        for a in EXTERNOS {
            let c = como_instalar_app(a.bin);
            assert!(c.starts_with("schematize-market install"), "{}: {c}", a.bin);
            assert!(!c.contains("curl"), "{}: {c}", a.bin);
        }
    }

    /// **O nome LEGADO é reconhecido.** Enquanto não era, o `status` e a aba do Mercado na
    /// GUI diziam "não instalado" sobre um deployer que estava na máquina — a resposta que
    /// manda a pessoa instalar o que já tem.
    #[test]
    fn descobrir_app_conhece_o_nome_legado() {
        assert_eq!(nome_legado("schematize-deployer"), Some("deployer"));
        assert_eq!(nome_legado("schematize-optimizer"), None);
        assert_eq!(nome_legado("schematize-market"), None);
        // O nome velho NUNCA é o novo — se virarem iguais, a busca de fallback vira ruído.
        for a in EXTERNOS {
            assert_ne!(nome_legado(a.bin), Some(a.bin));
        }
    }

    /// **A regra que a ponte existe para cumprir:** descobrir NUNCA falha. Numa máquina sem
    /// Deployer o resultado é `Ausente` — um estado —, não um erro que suba pro chamador.
    #[test]
    fn descobrir_nunca_falha_mesmo_sem_deployer() {
        // Não afirmamos QUAL estado (a máquina de quem roda pode ter o Deployer instalado);
        // afirmamos que a função retorna, sem panicar e sem `Result`.
        let e = descobrir();
        assert!(matches!(e, Estado::Instalado { .. } | Estado::Ausente | Estado::Quebrado { .. }));
    }
}
