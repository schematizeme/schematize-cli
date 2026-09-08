//! PIN DA GUI — qual commit de `schematize_gui_slint` um release publica.
//!
//! **O quê:** lê `packaging/gui-pin.txt`, o SHA que o `release.yml` clona para compilar o
//! binário `schematize-gui`. Nenhum código de produção usa isto em tempo de execução; existe
//! para que o pin seja **verificado por teste** em vez de ser um arquivo solto que ninguém
//! confere.
//!
//! **Onde:** os testes deste módulo, e o `release.yml` (job `linux`), que lê o mesmo arquivo.
//!
//! # Por que este arquivo existe
//!
//! O `release.yml` compilava a GUI assim:
//!
//! ```text
//! git clone --depth 1 https://github.com/schematizeme/schematize_gui_slint.git /tmp/gui
//! cargo update -p schematize
//! ```
//!
//! com um comentário dizendo *"usa o CÓDIGO desta tag, não o HEAD do main"* — que é o
//! contrário do que as duas linhas fazem. `clone --depth 1` sem `--branch` traz o **HEAD do
//! main** da GUI; `cargo update -p schematize` **descarta o `Cargo.lock` commitado** dela e
//! re-resolve o git-dep para o **HEAD do main do CLI**.
//!
//! Resultado: o binário `schematize-gui` de um release `vX` era feito do que estava no `main`
//! dos dois repos naquele instante. Recompilar a mesma tag amanhã dava outro binário, e não
//! havia como saber, a partir do release, que código foi publicado.
//!
//! # A correção, e por que NÃO foi `--locked`
//!
//! O óbvio seria compilar a GUI com `--locked`, respeitando o `Cargo.lock` dela. Seria
//! determinístico e **errado**: aquele lock pina o CLI num commit *anterior*, então o release
//! publicaria o CLI da tag ao lado de uma GUI ligada a um CLI mais velho — duas metades
//! diferentes no mesmo release.
//!
//! O que casa as duas metades é `cargo update -p schematize --precise <sha da tag>`: a GUI é
//! compilada contra o CLI **que está sendo publicado**. E o lado da GUI vira determinístico
//! pelo pin daqui, que faz a tag do CLI decidir, sozinha, o conteúdo inteiro do release.
//!
//! É o espelho do `lockpin` da GUI: lá o `Cargo.lock` fixa qual CLI ela usa; aqui o
//! `gui-pin.txt` fixa qual GUI o release publica. As duas direções pinadas, cada uma no repo
//! que decide.

/// O SHA pinado, sem espaço em volta. `None` se o arquivo não existir.
///
/// **Onde:** os testes abaixo. **De onde vem:** `packaging/gui-pin.txt`, relativo à raiz do
/// crate.
pub fn sha_da_gui() -> Option<String> {
    std::fs::read_to_string("packaging/gui-pin.txt").ok().map(|s| s.trim().to_string())
}

/// **O quê:** o pin está em dia com o `main` da GUI? `Err` com a mensagem acionável se não.
/// Função PURA — recebe os dois SHAs, não vai atrás deles.
///
/// **Onde:** o teste `o_pin_da_gui_esta_em_dia` (que busca o HEAD e chama isto) e o
/// self-check que força a falha conhecida.
///
/// **Por que a regra é uma função e não um `assert_eq!` dentro do teste:** um `assert_eq!` de
/// duas strings não pode ser exercitado no caminho VERMELHO — o self-check teria de comparar
/// dois literais e passaria mesmo se a comparação tivesse sumido do teste de verdade. Guard
/// que nunca é visto falhando é guard cego (§ "verde de verdade").
pub fn pin_esta_em_dia(pin: &str, head_do_main: &str) -> Result<(), String> {
    if pin == head_do_main {
        return Ok(());
    }
    Err(format!(
        "\n\nO pin da GUI está DESATUALIZADO.\n\
         \n  pinado: {pin}\n  main:   {head_do_main}\n\
         \nO release publicaria a GUI do commit pinado, não a mais recente — e nada mais\n\
         reprovaria isso: o fetch funciona, o build funciona, o asset sai. O sintoma chega\n\
         como \"atualizei e a janela continua igual\". Atualize com:\n\
         \n    git -C ../schematize_gui_slint rev-parse origin/main > packaging/gui-pin.txt\n\
         \nSe pinar uma versão ANTERIOR for deliberado, escreva o motivo no doc do módulo\n\
         `guipin` — é a única forma de a decisão sobreviver a quem a tomou."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O pin existe e é um SHA completo.
    ///
    /// **Por que SHA completo e não abreviado:** SHA curto colide, e um `git checkout` de
    /// prefixo ambíguo falha no meio do release — o pior lugar pra descobrir isso.
    #[test]
    fn pin_da_gui_e_sha_de_40_hex() {
        let sha = sha_da_gui().expect(
            "packaging/gui-pin.txt não existe — sem ele o release volta a compilar a GUI do \
             HEAD do main, e o binário deixa de ser reproduzível a partir da tag",
        );
        assert_eq!(sha.len(), 40, "SHA tem que ser completo (40 hex), veio {sha:?}");
        assert!(
            sha.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "SHA tem que ser hex minúsculo, veio {sha:?}"
        );
    }

    /// O arquivo tem UMA linha e nada mais — o `release.yml` o lê cru pra dentro de uma
    /// variável de shell, e um segundo campo viraria argumento perdido no `git fetch`.
    #[test]
    fn pin_tem_uma_linha_so() {
        let bruto = std::fs::read_to_string("packaging/gui-pin.txt").expect("o pin");
        assert_eq!(
            bruto.lines().count(),
            1,
            "o pin tem que ser uma linha só; comentário vai no doc deste módulo"
        );
        assert!(bruto.ends_with('\n'), "termina com newline (POSIX)");
    }

    /// **O pin está EM DIA com o `main` da GUI?**
    ///
    /// Este é o guard que faltava. Os testes acima provam que o pin tem a FORMA certa e que o
    /// commit EXISTE — nenhum deles nota que ele está parado. Um pin que nunca é bumpado
    /// publica, release após release, uma GUI de meses atrás, sem nenhum erro em lugar nenhum:
    /// o `git fetch` funciona, o build funciona, o asset sai. Só o usuário percebe, e como
    /// "atualizei e a janela continua igual".
    ///
    /// **Por que "igual ao HEAD" e não "ancestral de":** ancestral é o que já se sabe (o teste
    /// acima cobre a existência). A regra desta casa é que o release publica a GUI **verificada
    /// mais recente**; pinar deliberadamente uma anterior é decisão que precisa estar escrita
    /// em algum lugar, e este teste vermelho é o lugar onde ela aparece para ser escrita.
    ///
    /// **Ignorado por padrão, pelo mesmo motivo do teste acima:** atravessa fronteira de repo.
    /// Rodar com `cargo test -- --ignored`, e é o que o passo `pin da GUI em dia` do
    /// `ci.yml` chama depois de clonar o irmão.
    #[test]
    #[ignore = "precisa do schematize_gui_slint clonado ao lado; roda com --ignored"]
    fn o_pin_da_gui_esta_em_dia() {
        let sha = sha_da_gui().expect("o pin");
        let head = head_do_main_da_gui().expect(
            "não consegui ler o HEAD do main da GUI — o repo irmão precisa estar clonado ao lado",
        );
        if let Err(e) = pin_esta_em_dia(&sha, &head) {
            panic!("{e}");
        }
    }

    /// **O quê:** o SHA do `main` da GUI no repo irmão. `None` se ele não estiver ao lado.
    ///
    /// **Onde:** [`o_pin_da_gui_esta_em_dia`].
    ///
    /// Prefere `origin/main` ao `HEAD` local: o `HEAD` pode estar num branch de trabalho de
    /// quem desenvolve, e comparar o pin com um branch pessoal reprovaria por um motivo que
    /// não tem nada a ver com o release.
    fn head_do_main_da_gui() -> Option<String> {
        for referencia in ["origin/main", "main", "HEAD"] {
            let saida = std::process::Command::new("git")
                .args(["-C", "../schematize_gui_slint", "rev-parse", referencia])
                .output()
                .ok()?;
            if saida.status.success() {
                let s = String::from_utf8_lossy(&saida.stdout).trim().to_string();
                if s.len() == 40 {
                    return Some(s);
                }
            }
        }
        None
    }

    /// **O SELF-CHECK do guard, forçando a falha conhecida.**
    ///
    /// Um guard que nunca foi visto reprovando é um guard cego: se o `assert` sumisse do teste
    /// de cima, ele continuaria verde para sempre e ninguém notaria — que é exatamente o modo
    /// de falha que o pin desatualizado tem. Aqui a regra é exercitada nos DOIS sentidos.
    ///
    /// Roda SEMPRE (não é `#[ignore]`): a regra é pura e não depende do repo irmão.
    #[test]
    fn o_guard_do_pin_reprova_um_pin_atrasado() {
        let head = "83ce57fbc81a3921fe53a25c01555df84c35d8d8";

        // VERDE: pin igual ao main.
        assert!(pin_esta_em_dia(head, head).is_ok());

        // VERMELHO: qualquer outro SHA reprova — e a mensagem tem de ser acionável.
        let atrasado = "df48f1da0000000000000000000000000000abcd";
        let e = pin_esta_em_dia(atrasado, head).expect_err("um pin atrasado TEM de reprovar");
        assert!(e.contains(atrasado) && e.contains(head), "a mensagem tem de mostrar os dois: {e}");
        assert!(e.contains("gui-pin.txt"), "e o comando exato para consertar: {e}");
        assert!(e.contains("DESATUALIZADO"), "{e}");

        // E o caso que mais importa: o pin VAZIO (arquivo recém-criado) também reprova, em vez
        // de passar por coincidência de string.
        assert!(pin_esta_em_dia("", head).is_err());
    }

    /// O commit pinado EXISTE na GUI.
    ///
    /// **Ignorado por padrão:** depende do repo irmão estar clonado ao lado, o que é verdade
    /// na máquina de quem desenvolve e falso no CI, onde só este repo existe. Rodar com
    /// `cargo test -- --ignored` quando se quiser a checagem forte.
    ///
    /// A lição que este `#[ignore]` carrega: teste que atravessa fronteira de repo por
    /// caminho relativo não pode ser obrigatório — foi exatamente o que quebrou o primeiro
    /// run verde do CI desta esteira.
    #[test]
    #[ignore = "precisa do schematize_gui_slint clonado ao lado; roda com --ignored"]
    fn o_commit_pinado_existe_na_gui() {
        let sha = sha_da_gui().expect("o pin");
        let saida = std::process::Command::new("git")
            .args(["-C", "../schematize_gui_slint", "cat-file", "-t", &sha])
            .output()
            .expect("git");
        assert!(
            saida.status.success(),
            "o commit pinado {sha} não existe na GUI — o release falharia no `git fetch`"
        );
        assert_eq!(String::from_utf8_lossy(&saida.stdout).trim(), "commit");
    }
}
