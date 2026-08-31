//! O QUE: prova que nenhum caminho de ler-modificar-escrever destrói o arquivo do usuário
//! quando a LEITURA falha.
//!
//! POR QUE EXISTE: o idioma `read_to_string(p).unwrap_or_default()` seguido de
//! `fs::write(p, novo)` estava espalhado por sete pontos dos três crates, sempre sobre
//! arquivo do usuário — `.bashrc`, `~/.ssh/config`, `.gitignore`, `CHECKLIST.md`,
//! `DECISOES.md`. Ele mapeia TODA falha de leitura para "arquivo vazio" e então reescreve o
//! arquivo inteiro a partir do vazio: um erro de leitura **apaga** o conteúdo.
//!
//! E o gatilho não é exótico. `read_to_string` devolve `InvalidData` para qualquer arquivo
//! que não seja UTF-8 válido — um comentário acentuado em Latin-1 num `.bashrc` basta.
//!
//! DE ONDE VEM: arquivos temporários montados byte a byte. PRA ONDE VAI: só asserção.

use schematize::util::ler_para_modificar;

/// Sandbox exclusivo deste processo.
fn sandbox(nome: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("schematize-rmw-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Arquivo que não existe é estado NORMAL: devolve vazio pra quem vai criar.
#[test]
fn ausente_e_vazio_nao_erro() {
    let d = sandbox("ausente");
    assert_eq!(ler_para_modificar(&d.join("nao-existe")).unwrap(), "");
    let _ = std::fs::remove_dir_all(&d);
}

/// Conteúdo normal volta intacto — inclusive acentuado, desde que seja UTF-8.
#[test]
fn conteudo_valido_volta_inteiro() {
    let d = sandbox("valido");
    let p = d.join("rc");
    std::fs::write(&p, "# configuração antiga\nexport A=1\n").unwrap();
    assert_eq!(ler_para_modificar(&p).unwrap(), "# configuração antiga\nexport A=1\n");
    let _ = std::fs::remove_dir_all(&d);
}

/// **O caso que apagava arquivo.** `.bashrc` com acento em Latin-1 não é UTF-8 válido.
#[test]
fn nao_utf8_e_erro_e_nao_vazio() {
    let d = sandbox("latin1");
    let p = d.join(".bashrc");
    // "# configuração" em Latin-1: o 0xE7 (ç) e o 0xE3 (ã) são bytes inválidos em UTF-8.
    let latin1 = b"# configura\xE7\xE3o\nexport PATH=/opt/bin:$PATH\n";
    std::fs::write(&p, latin1).unwrap();

    let r = ler_para_modificar(&p);
    assert!(r.is_err(), "leitura de arquivo não-UTF-8 tem que FALHAR, não virar vazio");
    let e = r.unwrap_err();
    assert!(e.contains(".bashrc"), "o erro tem que dizer QUAL arquivo: {e}");
    assert!(e.contains("apagaria"), "o erro tem que dizer o que estava em jogo: {e}");

    // E o arquivo continua byte a byte como estava.
    assert_eq!(std::fs::read(&p).unwrap(), latin1, "o arquivo do usuário foi alterado");
    let _ = std::fs::remove_dir_all(&d);
}

/// Caminho que é diretório também é falha de leitura — nunca "vazio".
#[test]
fn diretorio_e_erro() {
    let d = sandbox("dir");
    let sub = d.join("sou-um-dir");
    std::fs::create_dir_all(&sub).unwrap();
    assert!(ler_para_modificar(&sub).is_err(), "diretório não pode virar string vazia");
    let _ = std::fs::remove_dir_all(&d);
}
