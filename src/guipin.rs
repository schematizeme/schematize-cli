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
