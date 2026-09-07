//! O QUE: prova que o `install.sh` coloca o dir de instalação no PATH dos terminais
//! futuros — escrevendo os `rc` ele mesmo, de forma idempotente e sem nunca truncar.
//!
//! POR QUE EXISTE: no openSUSE Leap 16 o `schematize` não existia no terminal depois de
//! uma instalação bem-sucedida. O `install.sh` não escrevia rc nenhum: ele herdava
//! `~/.cargo/bin` no PATH como EFEITO COLATERAL do rustup, que o `ensure_rust` só executa
//! quando `cargo` NÃO existe. Com `zypper install rust` o cargo mora em `/usr/bin`, o
//! rustup nunca roda, ninguém escreve o rc, e o binário do build do fonte fica inalcançável
//! — enquanto o instalador imprime "pronto. Próximos passos:" mandando digitar comandos
//! que não resolvem. Diagnóstico em `context/2026-09-02-opensuse-path-HANDOFF.md`.
//!
//! POR QUE EM RUST, CHAMANDO O SHELL DE VERDADE: o teste sourceia o `install.sh` real
//! (`SCHEMATIZE_INSTALL_LIB=1`) e invoca a `ensure_path_rc` original. Reimplementar a
//! lógica aqui não provaria nada sobre o script que a pessoa baixa, e recortar a função por
//! número de linha é a armadilha já conhecida (o dia em que erra é o dia em que isenta a
//! linha errada).
//!
//! DE ONDE VEM: `$HOME` falso montado byte a byte em diretório temporário.
//! PRA ONDE VAI: só asserção — nenhum arquivo fora do sandbox é lido ou escrito.
//!
//! POR QUE SÓ EM LINUX: o `install.sh` é Linux por desenho (`uname -s` != Linux → `die`).
//! Não é teste escondido de plataforma: nas outras não há o que testar.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

const DIR: &str = "/fake/home/.cargo/bin";

/// **O quê:** sandbox exclusivo deste teste, com `$HOME` próprio.
/// **Onde:** todo teste deste arquivo, uma vez cada.
fn sandbox(nome: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("schematize-pathrc-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// **O quê:** caminho do `install.sh` real, a partir da raiz do crate.
/// **Onde:** [`chama`]. Ancorado em `CARGO_MANIFEST_DIR` — não é caminho relativo
/// atravessando repo, que quebra quando o teste roda de outro diretório.
fn install_sh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh")
}

/// **O quê:** sourceia o `install.sh` com `$HOME` apontando pro sandbox e roda
/// `ensure_path_rc <dir>`. Devolve (sucesso, stdout+stderr).
///
/// **Onde:** todo teste deste arquivo. `SCHEMATIZE_INSTALL_NO_SELF=1` impede o script de
/// se re-baixar da rede; `SCHEMATIZE_INSTALL_LIB=1` faz ele definir as funções e parar,
/// sem instalar nada.
fn chama(home: &Path, dir: &str) -> (bool, String) {
    let sh = install_sh();
    let script = format!(". '{}' ; ensure_path_rc '{dir}'", sh.display());
    let out = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("HOME", home)
        .env("SCHEMATIZE_INSTALL_LIB", "1")
        .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
        .output()
        .expect("bash tem de existir para testar um script de shell");
    let mut txt = String::from_utf8_lossy(&out.stdout).into_owned();
    txt.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), txt)
}

/// **O quê:** bytes crus de um arquivo do sandbox (`Vec` vazio se não existe).
/// **Onde:** as asserções que precisam comparar byte a byte, não texto — o caso Latin-1.
fn bytes(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_default()
}

/// **O quê:** quantas vezes `DIR` aparece nos bytes do arquivo.
/// **Onde:** as asserções de idempotência.
fn ocorrencias(p: &Path) -> usize {
    String::from_utf8_lossy(&bytes(p)).matches(DIR).count()
}

// ---------------------------------------------------------------------------
// O bug do openSUSE, ao pé da letra.
// ---------------------------------------------------------------------------

/// O caso de campo: `.bashrc` existe e não menciona o dir → passa a mencionar, e o
/// conteúdo que já estava lá continua inteiro.
#[test]
fn escreve_no_bashrc_e_preserva_o_que_havia() {
    let d = sandbox("escreve");
    let rc = d.join(".bashrc");
    let antes = "# config do usuário\nexport EDITOR=vim\nalias ll='ls -la'\n";
    std::fs::write(&rc, antes).unwrap();

    let (ok, saida) = chama(&d, DIR);
    assert!(ok, "ensure_path_rc falhou: {saida}");

    let depois = String::from_utf8_lossy(&bytes(&rc)).into_owned();
    assert!(depois.contains(DIR), "o dir não entrou no .bashrc:\n{depois}");
    assert!(depois.starts_with(antes), "o conteúdo anterior não sobreviveu:\n{depois}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A regressão que apagou `.bashrc` de gente: rc que NÃO é UTF-8 válido. O idioma antigo
/// (`read_to_string(..).unwrap_or_default()` + reescrita) mapeava `InvalidData` para
/// "vazio" e regravava o arquivo contendo só o export. Em append isso não é possível —
/// e este teste é quem garante que o append continua sendo append.
#[test]
fn nao_trunca_rc_que_nao_e_utf8() {
    let d = sandbox("latin1");
    let rc = d.join(".bashrc");
    // "# configuração antiga" em Latin-1: o 0xE7 (ç) e o 0xE3 (ã) não são UTF-8 válido.
    let antes: Vec<u8> = b"# configura\xe7\xe3o antiga\nexport A=1\n".to_vec();
    std::fs::write(&rc, &antes).unwrap();

    let (ok, saida) = chama(&d, DIR);
    assert!(ok, "ensure_path_rc falhou: {saida}");

    let depois = bytes(&rc);
    assert!(
        depois.starts_with(&antes),
        "os bytes originais não sobreviveram — houve truncamento. antes={} depois={}",
        antes.len(),
        depois.len()
    );
    assert!(String::from_utf8_lossy(&depois).contains(DIR), "o dir não entrou");
    let _ = std::fs::remove_dir_all(&d);
}

/// Idempotência: o instalador roda em toda atualização. Duas passadas não podem deixar
/// duas linhas — senão o `.bashrc` de quem atualiza toda semana vira uma escada.
#[test]
fn duas_passadas_nao_duplicam() {
    let d = sandbox("idem");
    let rc = d.join(".bashrc");
    std::fs::write(&rc, "export A=1\n").unwrap();

    let (ok1, s1) = chama(&d, DIR);
    assert!(ok1, "1ª passada falhou: {s1}");
    let apos_uma = ocorrencias(&rc);
    assert_eq!(apos_uma, 1, "a 1ª passada devia escrever exatamente uma vez");

    let (ok2, s2) = chama(&d, DIR);
    assert!(ok2, "2ª passada falhou: {s2}");
    assert_eq!(ocorrencias(&rc), 1, "a 2ª passada duplicou a linha");
    let _ = std::fs::remove_dir_all(&d);
}

/// Linha posta À MÃO pelo usuário (formato diferente do nosso) também conta: não
/// acrescenta uma segunda. O alívio manual do handoff é exatamente esta linha.
#[test]
fn respeita_linha_posta_a_mao() {
    let d = sandbox("mao");
    let rc = d.join(".bashrc");
    std::fs::write(&rc, format!("PATH={DIR}:$PATH\nexport PATH\n")).unwrap();

    let (ok, saida) = chama(&d, DIR);
    assert!(ok, "falhou: {saida}");
    assert_eq!(ocorrencias(&rc), 1, "duplicou por cima do que o usuário já tinha feito");
    let _ = std::fs::remove_dir_all(&d);
}

/// `.profile` também é coberto — é o rc que o shell de LOGIN lê. O terminal gráfico lê
/// `.bashrc`, o login lê `.profile`; cobrir só um deixa metade dos usuários no escuro.
#[test]
fn cobre_bashrc_e_profile() {
    let d = sandbox("ambos");
    std::fs::write(d.join(".bashrc"), "export A=1\n").unwrap();
    std::fs::write(d.join(".profile"), "export B=2\n").unwrap();

    let (ok, saida) = chama(&d, DIR);
    assert!(ok, "falhou: {saida}");
    assert_eq!(ocorrencias(&d.join(".bashrc")), 1, ".bashrc ficou de fora");
    assert_eq!(ocorrencias(&d.join(".profile")), 1, ".profile ficou de fora");
    let _ = std::fs::remove_dir_all(&d);
}

/// Não inventa config de shell que o usuário não usa: `.zshrc` ausente continua ausente.
/// (`.bashrc` e `.profile` são padrão e podem nascer aqui — `.zshrc` não.)
#[test]
fn nao_cria_zshrc_para_quem_nao_usa_zsh() {
    let d = sandbox("zsh");
    std::fs::write(d.join(".bashrc"), "export A=1\n").unwrap();

    let (ok, saida) = chama(&d, DIR);
    assert!(ok, "falhou: {saida}");
    assert!(!d.join(".zshrc").exists(), "criou um .zshrc para quem não tem zsh");
    // Mas se ele JÁ existe, é coberto.
    std::fs::write(d.join(".zshrc"), "export Z=1\n").unwrap();
    let (ok2, s2) = chama(&d, DIR);
    assert!(ok2, "falhou: {s2}");
    assert_eq!(ocorrencias(&d.join(".zshrc")), 1, ".zshrc existente ficou de fora");
    let _ = std::fs::remove_dir_all(&d);
}

/// ASSERÇÃO NEGATIVA: dir que já está em todo PATH não gera export nenhum. Sem isto, um
/// `ensure_path_rc` que escrevesse SEMPRE passaria em todos os testes acima.
#[test]
fn dir_de_sistema_nao_suja_rc_nenhum() {
    let d = sandbox("sistema");
    let rc = d.join(".bashrc");
    let antes = "export A=1\n";
    std::fs::write(&rc, antes).unwrap();

    let (ok, saida) = chama(&d, "/usr/bin");
    assert!(ok, "falhou: {saida}");
    assert_eq!(bytes(&rc), antes.as_bytes(), "escreveu export para /usr/bin, que todo PATH já tem");
    let _ = std::fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------------------
// Guardas do próprio script — defeitos que já existiam e passariam calados.
// ---------------------------------------------------------------------------

/// `warn` era chamada em quatro pontos e nunca definida. Sob `set -euo pipefail` o nome
/// não resolve, o shell sai 127 e a instalação ABORTA — o aviso matava o instalador.
/// `bash -n` não pega isto (é resolução em tempo de execução), e o shellcheck também não.
#[test]
fn os_helpers_de_log_existem_todos() {
    let sh = install_sh();
    for f in ["log", "ok", "die", "warn"] {
        let script = format!(". '{}' >/dev/null 2>&1 ; type {f}", sh.display());
        let out = Command::new("bash")
            .arg("-c")
            .arg(&script)
            .env("SCHEMATIZE_INSTALL_LIB", "1")
            .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "`{f}` é usada no install.sh mas não está definida");
    }
}

/// SELF-CHECK: o arnês tem de ser capaz de REPROVAR. Um `chama()` que sempre devolvesse
/// sucesso deixaria os sete testes acima cegos — foi assim que um helper falso já fez dois
/// testes de trava não afirmarem nada. Aqui uma função inexistente TEM de falhar.
#[test]
fn o_arnes_consegue_ver_falha() {
    let sh = install_sh();
    let script = format!(". '{}' >/dev/null 2>&1 ; funcao_que_nao_existe", sh.display());
    let out = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("SCHEMATIZE_INSTALL_LIB", "1")
        .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
        .output()
        .unwrap();
    assert!(!out.status.success(), "o arnês reportou sucesso para função inexistente — está cego");
}

// ---------------------------------------------------------------------------
// A flag `--deployer` (ADR-0010) — opt-in, e o opt-in é a asserção.
// ---------------------------------------------------------------------------

/// `--deployer` liga o flag; **sem ele o flag fica desligado**. A segunda metade é a que
/// importa: o Deployer saiu do fluxo principal de propósito, e enquanto o `schematize` ainda
/// tem `ssh` e `vps` embutidos, instalá-lo por padrão entregaria a mesma funcionalidade duas
/// vezes por dois comandos — a ambiguidade que a `purge_previous` deste script existe para
/// matar.
#[test]
fn a_flag_deployer_e_opt_in() {
    let sh = install_sh();
    let ver = |args: &str| -> String {
        let script = format!(". '{}' >/dev/null 2>&1 ; echo \"$DEPLOYER\"", sh.display());
        let out = Command::new("bash")
            .arg("-c")
            .arg(format!("set -- {args}; {script}"))
            .env("SCHEMATIZE_INSTALL_LIB", "1")
            .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(ver(""), "0", "sem a flag, o deployer NÃO pode ser instalado");
    assert_eq!(ver("--deployer"), "1", "com a flag, liga");
    // Combina com os outros modos sem atrapalhá-los.
    assert_eq!(ver("--binary --deployer"), "1");
}

/// O bloco do deployer é **best-effort**: o `install.sh` não pode morrer porque um app
/// OPCIONAL não compilou. Quem pediu `--deployer` não deixou de querer o schematize.
#[test]
fn a_falha_do_deployer_nao_derruba_o_install() {
    let txt = std::fs::read_to_string(install_sh()).unwrap();
    let i = txt.find("compilando o schematize-deployer").expect("o bloco do deployer");
    let bloco = &txt[i..(i + 900).min(txt.len())];
    assert!(bloco.contains("warn "), "a falha tem de AVISAR, não passar calada");
    assert!(
        !bloco.contains("die "),
        "o deployer é opcional: `die` aqui derrubaria a instalação do schematize junto"
    );
}

/// **A purga NÃO pode levar o deployer.** Ela roda em toda instalação; com o `deployer` na
/// lista, um `install.sh` sem `--deployer` — o caso normal — apagaria o deployer de quem o
/// tem. Instalar um app não pode desinstalar outro, pelo mesmo motivo que atualizar não pode
/// instalar o que ninguém pediu.
#[test]
fn a_purga_nao_leva_o_deployer_junto() {
    let txt = std::fs::read_to_string(install_sh()).unwrap();
    let i = txt.find("BINS=").expect("a lista de purga");
    let lista: String = txt[i..].lines().take(2).collect::<Vec<_>>().join(" ");
    assert!(lista.contains("schematize-gui"), "sanidade: é a lista certa? {lista}");
    assert!(
        !lista.contains("deployer"),
        "o `deployer` entrou na purga — todo install sem --deployer passaria a apagá-lo: {lista}"
    );
}
