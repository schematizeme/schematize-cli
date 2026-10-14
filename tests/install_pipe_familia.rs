//! O QUE: prova quatro regressões do `install.sh` vistas numa instalação REAL no CachyOS
//! (`curl .../install.sh | bash -s -- --completo`): (1) família `arch` não detectada;
//! (2) `registrar_no_menu: comando não encontrado` — a função estava ANINHADA dentro de
//! `install_source`, logo só existia no modo fonte; (3) `curl: (23) Failure writing output` —
//! a auto-atualização faz `exec` num script baixado e o bash de fora, que lia o pipe, sumia
//! sem consumir o resto; (4) o script nunca pode estar meio-definido.
//!
//! POR QUE EM RUST COM FALSOS NO PATH: roda o `install.sh` REAL, alimentado por pipe (como o
//! `curl | bash`), com `curl`/`uname`/`sudo` falsos e `HOME` de sandbox. Nada toca a rede.
//!
//! DE ONDE VEM: sandbox em diretório temporário. PRA ONDE VAI: só asserções.
#![cfg(target_os = "linux")]

use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const REAIS: &[&str] = &[
    "env", "id", "mkdir", "mktemp", "rm", "cp", "mv", "chmod", "install", "cat", "grep", "sed",
    "head", "tail", "tr", "awk", "sort", "cut", "basename", "dirname", "date", "touch", "ls",
    "getent", "logname", "tty", "stat", "pkill", "ln", "wc", "sleep", "sh", "bash", "readlink",
    "uniq", "find", "xargs", "dpkg", "rpm", "tee", "true", "uname", "du",
];

fn install_sh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh")
}

/// **O quê:** sandbox com `bin/` (PATH), `home/`, `log/` e `tmp/`. **Onde:** todo teste daqui.
struct Box {
    raiz: PathBuf,
}

impl Box {
    fn novo(nome: &str) -> Box {
        let raiz =
            std::env::temp_dir().join(format!("schematize-pipe-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        for d in ["bin", "home", "log", "tmp"] {
            fs::create_dir_all(raiz.join(d)).unwrap();
        }
        let b = Box { raiz };
        for t in REAIS {
            for dir in ["/usr/bin", "/bin"] {
                let src = Path::new(dir).join(t);
                if src.exists() {
                    let _ = symlink(&src, b.raiz.join("bin").join(t));
                    break;
                }
            }
        }
        b
    }
    fn falso(&self, nome: &str, corpo: &str) {
        let p = self.raiz.join("bin").join(nome);
        let _ = fs::remove_file(&p);
        fs::write(&p, format!("#!/bin/sh\n{corpo}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    /// `curl` falso: registra a URL; em `-o <arq>` grava `conteudo_baixado` (ou um binário falso).
    fn curl(&self, baixado: &str) {
        let corpo = format!(
            r#"url=""; out=""; prev=""
for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; url="$a"; done
echo "$url" >> "$LOGDIR/fetch.log"
case "$url" in *api.github.com*) exit 22;; esac
[ -z "$out" ] && exit 0
[ "$out" = "/dev/null" ] && exit 0
case "$url" in
  *raw.githubusercontent.com*install.sh) printf '%s\n' '{baixado}' > "$out";;
  *) base="${{url##*/}}"
     printf '#!/bin/sh\necho "%s $*" >> "%s/calls.log"\ncase "$1" in --version) echo "%s 0.99.0 (fake)";; esac\nexit 0\n' "$base" "$LOGDIR" "$base" > "$out"
     chmod +x "$out";;
esac"#
        );
        self.falso("curl", &corpo);
    }
    fn log(&self, nome: &str) -> String {
        fs::read_to_string(self.raiz.join("log").join(nome)).unwrap_or_default()
    }
    /// Roda o `install.sh` ALIMENTADO POR PIPE (`bash -s -- args`), como o `curl | bash`.
    /// Devolve (saída combinada, resultado do `write_all` no pipe, arquivos em tmp/).
    fn pipe(&self, args: &[&str], com_self_update: bool) -> (String, std::io::Result<()>, usize) {
        self.pipe_bytes(fs::read(install_sh()).unwrap(), args, com_self_update)
    }
    /// Igual a [`Box::pipe`], mas com o conteúdo do script dado (ex.: truncado).
    fn pipe_bytes(
        &self,
        script: Vec<u8>,
        args: &[&str],
        com_self_update: bool,
    ) -> (String, std::io::Result<()>, usize) {
        let mut c = Command::new("/bin/bash");
        c.args(["-s", "--"])
            .args(args)
            .env_clear()
            .env("PATH", self.raiz.join("bin"))
            .env("HOME", self.raiz.join("home"))
            .env("LOGDIR", self.raiz.join("log"))
            .env("TMPDIR", self.raiz.join("tmp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !com_self_update {
            c.env("SCHEMATIZE_INSTALL_NO_SELF", "1");
        }
        let mut filho = c.spawn().unwrap();
        let mut stdin = filho.stdin.take().unwrap();
        // thread alimentadora: se o leitor sumir sem drenar, write_all devolve EPIPE.
        let feeder = std::thread::spawn(move || stdin.write_all(&script));
        let o = filho.wait_with_output().unwrap();
        let w = feeder.join().unwrap();
        let t =
            format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        let tmp = fs::read_dir(self.raiz.join("tmp")).unwrap().count();
        (t, w, tmp)
    }
}

/// **O quê:** `FAMILY` que o script calcula para um `/etc/os-release` falso.
fn familia(nome: &str, os_release: &str) -> String {
    let d = std::env::temp_dir().join(format!("schematize-fam-{nome}-{}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    let f = d.join("os-release");
    fs::write(&f, os_release).unwrap();
    let cmd = format!(". '{}' >/dev/null 2>&1; printf %s \"$FAMILY\"", install_sh().display());
    let o = Command::new("bash")
        .args(["-c", &cmd])
        .env("SCHEMATIZE_OS_RELEASE", &f)
        .env("SCHEMATIZE_INSTALL_LIB", "1")
        .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn familia_por_id_like_cobre_derivadas() {
    let casos = [
        ("cachyos", "ID=cachyos\nID_LIKE=arch\nPRETTY_NAME=\"CachyOS\"\n", "arch"),
        ("manjaro", "ID=manjaro\nID_LIKE=arch\n", "arch"),
        ("endeavouros", "ID=endeavouros\nID_LIKE=arch\n", "arch"),
        ("garuda", "ID=garuda\nID_LIKE=arch\n", "arch"),
        ("archpuro", "ID=arch\n", "arch"),
        ("pop", "ID=pop\nID_LIKE=\"ubuntu debian\"\n", "debian"),
        ("linuxmint", "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n", "debian"),
        ("tumbleweed", "ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n", "rpm"),
        ("fedora", "ID=fedora\n", "rpm"),
        ("desconhecida", "ID=nixos\n", "unknown"),
    ];
    for (n, conteudo, esperado) in casos {
        assert_eq!(familia(n, conteudo), esperado, "distro {n}");
    }
}

#[test]
fn instalacao_por_pipe_no_modo_binario_nao_tem_comando_nao_encontrado() {
    let b = Box::novo("binario");
    b.curl("#!/bin/bash\necho NOVO");
    b.falso("sudo", "[ \"$1\" = -n ] && shift\nexec \"$@\"");
    let (t, w, _) = b.pipe(&["--binary", "--completo"], false);
    for ruim in ["comando não encontrado", "command not found"] {
        assert!(!t.contains(ruim), "saiu '{ruim}' no modo binário:\n{t}");
    }
    assert!(w.is_ok(), "pipe quebrou: {w:?}");
    assert!(
        t.contains("já aparece no menu") || t.contains("não consegui pôr o ícone"),
        "registrar_no_menu nunca rodou:\n{t}"
    );
}

#[test]
fn auto_atualizacao_por_pipe_drena_o_stdin_sem_epipe() {
    let b = Box::novo("selfupdate");
    b.curl("#!/bin/bash\necho SCRIPT_NOVO_RODOU");
    let (t, w, _) = b.pipe(&["--binary"], true);
    assert!(t.contains("SCRIPT_NOVO_RODOU"), "o exec do script novo não aconteceu:\n{t}");
    assert!(fs::metadata(install_sh()).unwrap().len() > 70_000, "script cabe no pipe: teste cego");
    assert!(w.is_ok(), "o alimentador do pipe tomou EPIPE (curl 23 no mundo real): {w:?}\n{t}");
}

#[test]
fn self_check_sem_drenar_o_alimentador_tomaria_epipe() {
    // Prova que o teste acima NÃO é cego: um leitor que sai sem consumir causa EPIPE.
    let mut filho = Command::new("bash")
        .args(["-c", "head -c 10 >/dev/null; exit 0"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = filho.stdin.take().unwrap();
    let r = stdin.write_all(&vec![b'x'; 1 << 20]);
    let _ = filho.wait();
    assert!(r.is_err(), "esperava EPIPE do leitor que não drena");
}

#[test]
fn sem_funcao_aninhada_nem_chamada_antes_da_definicao() {
    // Função definida DENTRO de outra só existe depois que a de fora roda (o bug do
    // registrar_no_menu). Proíbe definição indentada e exige def antes da 1ª chamada top-level.
    let src = fs::read_to_string(install_sh()).unwrap();
    let mut em_heredoc = false;
    // O preâmbulo POSIX (antes do `set -euo pipefail`) define helpers dentro de `if`, sem função externa.
    let ini = src.lines().position(|l| l == "set -euo pipefail").unwrap();
    for (i, l) in src.lines().enumerate().skip(ini) {
        if l.contains("<<'") || l.contains("<< '") || l.contains("<<EOF") {
            em_heredoc = true;
        }
        if em_heredoc {
            if l.trim() == "EOF" || l.trim() == "SH" {
                em_heredoc = false;
            }
            continue;
        }
        let t = l.trim_start();
        if l.starts_with(' ')
            && t.split_once("() {").is_some_and(|(n, _)| {
                !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
        {
            panic!("função aninhada (só existe após a de fora rodar): linha {}: {l}", i + 1);
        }
    }
    let def = src.find("\nregistrar_no_menu() {").expect("definição top-level");
    let install_source = src.find("\ninstall_source() {").unwrap();
    let kit = src.find("\nkit_completo() {").unwrap();
    assert!(
        def < install_source && def < kit,
        "registrar_no_menu deve ser definida antes dos usos"
    );
}

#[test]
fn download_truncado_nao_executa_nada() {
    let completo = fs::read(install_sh()).unwrap();
    assert!(
        String::from_utf8_lossy(&completo).trim_end().ends_with("main \"$@\""),
        "main deve ser a última linha"
    );
    for (nome, pct) in [("t50", 50usize), ("t90", 90)] {
        let b = Box::novo(nome);
        b.curl("#!/bin/bash\necho SCRIPT_NOVO_RODOU");
        b.falso("sudo", "echo sudo >> \"$LOGDIR/sudo.log\"; exit 0");
        let corte = completo.len() * pct / 100;
        let (t, _w, tmp) =
            b.pipe_bytes(completo[..corte].to_vec(), &["--binary", "--completo"], true);
        let home = fs::read_dir(b.raiz.join("home")).unwrap().count();
        assert_eq!(home, 0, "{pct}%: escreveu no HOME:\n{t}");
        assert_eq!(tmp, 0, "{pct}%: criou temporário:\n{t}");
        assert!(b.log("fetch.log").is_empty(), "{pct}%: baixou algo:\n{t}");
        assert!(b.log("sudo.log").is_empty(), "{pct}%: chamou sudo");
        assert!(!t.contains("SCRIPT_NOVO_RODOU") && !t.contains("▶"), "{pct}%: executou:\n{t}");
    }
}

/// Roda o script com `uname -s` falso; devolve (código, saída, entradas em HOME/tmp/log de fetch).
fn com_uname(nome: &str, sistema: &str) -> (Option<i32>, String, usize, String) {
    let b = Box::novo(nome);
    b.curl("#!/bin/bash\necho SCRIPT_NOVO_RODOU");
    b.falso("uname", &format!("echo {sistema}"));
    let (t, _w, tmp) = b.pipe(&["--completo"], true);
    let home = fs::read_dir(b.raiz.join("home")).unwrap().count();
    (None, t, tmp + home, b.log("fetch.log"))
}

#[test]
fn windows_git_bash_aponta_o_instalador_ps1_e_nao_toca_em_nada() {
    for sis in ["MINGW64_NT-10.0-22631", "MSYS_NT-10.0", "CYGWIN_NT-10.0"] {
        let (_, t, efeitos, fetch) = com_uname("win", sis);
        assert!(t.contains("irm https://raw.githubusercontent.com/schematizeme/schematize_windows_rs/main/install.ps1 | iex"), "{sis}:\n{t}");
        assert!(!t.contains("só Linux") && !t.contains("SCRIPT_NOVO_RODOU"), "{sis}:\n{t}");
        assert_eq!(efeitos, 0, "{sis}: tocou no disco");
        assert!(fetch.is_empty(), "{sis}: baixou algo");
    }
}

#[test]
fn macos_da_mensagem_acionavel_sem_tocar_em_nada() {
    let (_, t, efeitos, fetch) = com_uname("mac", "Darwin");
    assert!(t.contains("cargo install") && t.contains("nada foi alterado"), "{t}");
    assert_eq!(efeitos, 0);
    assert!(fetch.is_empty());
}
