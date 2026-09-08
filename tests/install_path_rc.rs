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

/// A instalação do deployer é **best-effort**: o `install.sh` não pode morrer porque um app
/// OPCIONAL não veio. Quem pediu `--deployer` não deixou de querer o schematize.
///
/// **A âncora mudou com o ADR-0013, o invariante não.** O script deixou de COMPILAR o
/// deployer — quem o instala agora é o `schematize-market`, e este arquivo só delega
/// (`delega_ao_market`). O que continua valendo, e é o ponto do teste, é que essa delegação
/// AVISA quando falha e nunca derruba a instalação junto.
#[test]
fn a_falha_do_deployer_nao_derruba_o_install() {
    let txt = std::fs::read_to_string(install_sh()).unwrap();

    // 1) A função que delega existe, avisa e não mata.
    let i = txt.find("delega_ao_market() {").expect("a função que delega ao gestor");
    let resto = &txt[i..];
    let bloco = match resto.find("\n}\n") {
        Some(fim) => &resto[..fim],
        None => resto,
    };
    assert!(bloco.contains("warn "), "a falha tem de AVISAR, não passar calada");
    assert!(
        !bloco.contains("die "),
        "o app delegado é opcional: `die` aqui derrubaria a instalação do schematize junto"
    );
    // A mensagem tem de ser acionável: dizer o comando para repetir à mão (§37.48).
    assert!(
        bloco.contains("schematize-market install"),
        "a mensagem de falha precisa dar o comando para tentar de novo"
    );

    // 2) E o script NÃO compila mais o deployer — se voltar a compilar, voltam os dois
    //    caminhos para a mesma coisa que o ADR-0013 existe para acabar.
    assert!(
        !txt.contains("schematize_deployer_rs.git"),
        "o install.sh voltou a clonar o deployer — o dono da instalação é o market"
    );
    assert!(
        !txt.contains("schematize_optimizer_rs.git"),
        "o install.sh voltou a clonar o optimizer — o dono da instalação é o market"
    );
}

/// **O LOOP, travado pelo lado do script.** O `install.sh --deployer` delega ao market, e o
/// `market install <app>` compila do fonte. Se algum dia o market voltar a chamar este script,
/// os dois se chamam em círculo — por isso o teste vive nos DOIS lados (o outro está em
/// `schematize_market_rs::appsdacasa`).
#[test]
fn a_delegacao_nao_reentra_no_proprio_script() {
    let txt = std::fs::read_to_string(install_sh()).unwrap();
    let i = txt.find("delega_ao_market() {").expect("a função que delega ao gestor");
    let resto = &txt[i..];
    let bloco = match resto.find("\n}\n") {
        Some(fim) => &resto[..fim],
        None => resto,
    };
    for proibido in ["curl ", "install.sh", "RAW_INSTALL"] {
        assert!(!bloco.contains(proibido), "`{proibido}` na delegação reabre o loop");
    }
}

/// **TODA função chamada no `install.sh` está DEFINIDA nele.**
///
/// # O bug real que este teste existe para não repetir
///
/// O ADR-0013 removeu a função `install_updater`. Ficou uma chamada a ela no `post_config` —
/// que é o caminho por onde TODA instalação passa. O sintoma, numa instalação limpa de
/// verdade, foi uma linha no fim de tudo:
///
/// ```text
/// /tmp/install.sh: line 732: install_updater: command not found
/// ```
///
/// **Nenhum gate pegou isso.** `bash -n` não pega: chamar função inexistente é sintaticamente
/// válido. O `shellcheck` não pega: para ele é um comando externo que talvez exista no PATH.
/// A bateria de shells não pega: ela testa o shim do ops, não este arquivo. Só a instalação
/// limpa em container pegou — depois de vinte minutos compilando.
///
/// Pior que a mensagem: sob `set -e` a chamada estava com `|| true`, então ela **não abortava
/// nada**. A instalação terminava dizendo "pronto" com um erro no meio que ninguém leria — e
/// se aquela chamada fosse o único ponto que instalava o gestor (era, nos caminhos `--binary`
/// e `package`), a máquina terminaria sem gestor, em silêncio. É o risco R3 inteiro numa
/// linha.
///
/// # Como ele funciona
///
/// Extrai os nomes DEFINIDOS (`nome() {`) e os nomes CHAMADOS que parecem função nossa —
/// identificador com `_`, no início de um comando. Um chamado que não está definido reprova.
/// Só nomes com `_` de propósito: é a convenção deste arquivo, e evita ter de manter uma
/// allowlist de todo binário do sistema (`grep`, `curl`, `install`…), que é a lista que
/// envelhece e cria falso positivo.
#[test]
fn nao_ha_chamada_a_funcao_que_nao_existe() {
    let txt = std::fs::read_to_string(install_sh()).unwrap();

    // A DEFINIÇÃO pode ter comentário ou corpo depois do `{` (`f() { # nota` e
    // `log() { printf …; }` são os dois casos reais aqui), então a marca é o `() {`, não o
    // fim da linha. Foi este detalhe que o teste errou na primeira escrita: sete funções que
    // EXISTEM apareceram como órfãs.
    let mut definidas: Vec<String> = Vec::new();
    for l in txt.lines() {
        let l = l.trim();
        if let Some(i) = l.find("() {") {
            let nome = &l[..i];
            if !nome.is_empty() && nome.chars().all(|c| c.is_alphanumeric() || c == '_') {
                definidas.push(nome.to_string());
            }
        }
    }
    assert!(definidas.len() > 10, "não achei as funções do script: {definidas:?}");

    let mut orfas: Vec<(usize, String)> = Vec::new();
    for (n, linha) in txt.lines().enumerate() {
        let l = linha.trim();
        // Comentário e definição não são chamada.
        if l.starts_with('#') || l.ends_with("() {") {
            continue;
        }
        // O primeiro token de um comando, inclusive depois de `if`, `&&`, `||`, `;`, `!`.
        for pedaco in l.split(['|', '&', ';']) {
            // Só o PRIMEIRO token que não seja palavra-chave de shell: é ele que nomeia o
            // comando. O `break` no fim do corpo garante isso.
            for t in pedaco.split_whitespace() {
                if matches!(t, "if" | "then" | "else" | "elif" | "!" | "while" | "until" | "do") {
                    continue;
                }
                // `$mv_` é EXPANSÃO DE VARIÁVEL no lugar do comando (o script monta o `mv` com
                // ou sem `sudo` assim). O valor dela é decidido em runtime e não há função
                // nossa para procurar.
                if t.starts_with('$') || t.starts_with("\"$") {
                    break;
                }
                let nome = t.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                if nome.contains('_')
                    && nome.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !nome.chars().next().is_some_and(|c| c.is_ascii_digit())
                    // Parece nome de função nossa e NÃO está definido nem é variável.
                    && !definidas.iter().any(|d| d == nome)
                    // Variáveis são MAIÚSCULAS neste script; funções, minúsculas.
                    && nome.chars().any(|c| c.is_lowercase())
                    && !nome.chars().any(|c| c.is_uppercase())
                    // Nomes conhecidos que NÃO são função deste script.
                    && !matches!(nome, "update_desktop_database" | "gtk_update_icon_cache")
                {
                    orfas.push((n + 1, nome.to_string()));
                }
                break; // só o PRIMEIRO token de cada comando é o nome
            }
        }
    }
    assert!(
        orfas.is_empty(),
        "chamada(s) a função que NÃO existe no install.sh — é o `install_updater: command not \
         found` de novo: {orfas:?}"
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
