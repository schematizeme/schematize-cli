//! O QUE: prova o suporte a FreeBSD do `install.sh` — preâmbulo POSIX que traz o bash por `pkg`,
//! download por `fetch` quando não há `curl`, mapa FreeBSD/amd64 → assets `freebsd-x86_64`, zero
//! `sudo` à toa — e a REGRESSÃO do caminho Linux (assets `linux-x86_64` pelo `curl`).
//!
//! POR QUE EM RUST, COM `uname`/`fetch`/`curl`/`pkg`/`sudo` FALSOS NO PATH: não há FreeBSD na CI
//! de testes; o que dá pra provar é o COMPORTAMENTO do script real diante da plataforma que o
//! `uname` declara, observando quais URLs foram pedidas e que privilégio foi invocado. O binário
//! de verdade roda na VM FreeBSD do release (`release.yml`) e na máquina de quem instala.
//!
//! DE ONDE VEM: sandbox em diretório temporário (`HOME` próprio, PATH só com symlinks mínimos).
//! PRA ONDE VAI: só asserção sobre os logs dos falsos — nada sai do sandbox nem toca a rede.
#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Ferramentas reais que o script usa e que entram (por symlink) no PATH do sandbox. `curl`,
/// `fetch`, `sudo`, `doas`, `pkg` e `uname` NÃO estão aqui: cada teste decide o que existe.
const REAIS: &[&str] = &[
    "env", "id", "mkdir", "mktemp", "rm", "cp", "mv", "chmod", "install", "cat", "grep", "sed",
    "head", "tail", "tr", "awk", "sort", "cut", "basename", "dirname", "date", "touch", "ls",
    "getent", "logname", "tty", "stat", "pkill", "ln", "wc", "sleep", "sh", "readlink", "uniq",
    "find", "xargs", "dpkg", "rpm", "tee", "true",
];

/// **O quê:** sandbox com `bin/` (PATH), `home/` e `log/`. **Onde:** todo teste deste arquivo.
struct Box {
    raiz: PathBuf,
}

impl Box {
    fn novo(nome: &str) -> Box {
        let raiz =
            std::env::temp_dir().join(format!("schematize-fbsd-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        for d in ["bin", "home", "log"] {
            fs::create_dir_all(raiz.join(d)).unwrap();
        }
        let b = Box { raiz };
        for t in REAIS {
            for dir in ["/usr/bin", "/bin"] {
                let src = Path::new(dir).join(t);
                if src.exists() {
                    let _ = symlink(&src, b.bin().join(t));
                    break;
                }
            }
        }
        b
    }
    fn bin(&self) -> PathBuf {
        self.raiz.join("bin")
    }
    /// Diretório que faz as vezes de `/usr/local/bin` (vazio por padrão).
    fn sys(&self) -> PathBuf {
        let d = self.raiz.join("sys");
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn log(&self, nome: &str) -> String {
        fs::read_to_string(self.raiz.join("log").join(nome)).unwrap_or_default()
    }
    /// Instala um executável falso (script `sh`) em `bin/`.
    fn falso(&self, nome: &str, corpo: &str) {
        let p = self.bin().join(nome);
        // remove antes: `nome` pode ser symlink pra ferramenta REAL (id) e write seguiria o link.
        let _ = fs::remove_file(&p);
        fs::write(&p, format!("#!/bin/sh\n{corpo}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn uname(&self, sistema: &str, arch: &str) {
        self.falso(
            "uname",
            &format!(
                "case \"$1\" in -s) echo {sistema};; -m) echo {arch};; -r) echo 14.2-RELEASE;; *) echo {sistema};; esac"
            ),
        );
    }
    /// `curl`/`fetch` falso: registra a URL e grava, em `-o`, um executável que registra a chamada.
    fn baixador(&self, nome: &str) {
        let corpo = r#"url=""; out=""; prev=""
for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; url="$a"; done
echo "$url" >> "$LOGDIR/fetch.log"
case "$url" in *api.github.com*) exit 22;; esac
[ -z "$out" ] && exit 0
[ "$out" = "-" ] && exit 22
[ "$out" = "/dev/null" ] && exit 0
base="${url##*/}"
printf '#!/bin/sh\necho "%s $*" >> "%s/calls.log"\ncase "$1" in --version) echo "%s 0.99.0 (fake)";; esac\nexit 0\n' "$base" "$LOGDIR" "$base" > "$out"
chmod +x "$out""#;
        self.falso(nome, corpo);
    }
    /// `sudo`/`doas` falso: registra e roda o comando, como o real faria.
    fn escalador(&self, nome: &str) {
        self.falso(
            nome,
            &format!(
                "echo \"{nome} $*\" >> \"$LOGDIR/sudo.log\"\n[ \"$1\" = -n ] && shift\nexec \"$@\""
            ),
        );
    }
    /// Roda `bash install.sh <args>` com PATH = só o sandbox.
    fn instala(&self, args: &[&str]) -> Output {
        let sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
        Command::new("/bin/bash")
            .arg(sh)
            .args(args)
            .env_clear()
            .env("PATH", self.bin())
            .env("HOME", self.raiz.join("home"))
            .env("LOGDIR", self.raiz.join("log"))
            // HERMÉTICO: o script consulta (e manda `sudo rm`) cópias antigas em /usr/local/bin e
            // /usr/bin. Aponta pra um dir do sandbox, senão o teste depende do que a máquina tem.
            .env("SCHEMATIZE_SYS_BIN_DIRS", self.sys())
            .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
            .output()
            .expect("bash roda")
    }
}

fn texto(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Linhas do sudo.log que não são a sonda `-n true` do `sudo_utilizavel`.
fn usos_de_escalador(b: &Box) -> Vec<String> {
    b.log("sudo.log").lines().filter(|l| !l.ends_with("-n true")).map(String::from).collect()
}

#[test]
fn freebsd_amd64_sem_curl_baixa_assets_freebsd_com_fetch_e_sem_sudo() {
    let b = Box::novo("amd64");
    b.uname("FreeBSD", "amd64");
    b.baixador("fetch");
    b.escalador("sudo");
    b.falso("pkg", "echo \"pkg $*\" >> \"$LOGDIR/pkg.log\"\nexit 0"); // `pkg info -e` = presente
    let o = b.instala(&["--binary", "--completo"]);
    let t = texto(&o);
    let f = b.log("fetch.log");
    for esperado in [
        "/releases/latest/download/schematize-freebsd-x86_64",
        "/releases/latest/download/schematize-gui-freebsd-x86_64",
        "/schematize_market_rs/releases/latest/download/schematize-market-freebsd-x86_64",
    ] {
        assert!(f.contains(esperado), "faltou {esperado}\nfetch.log:\n{f}\nsaida:\n{t}");
    }
    assert!(!f.contains("linux"), "pediu asset linux no FreeBSD:\n{f}");
    assert!(b.raiz.join("home/.cargo/bin/schematize").is_file(), "hub não instalado\n{t}");
    assert!(
        b.raiz.join("home/.cargo/bin/schematize-market").is_file(),
        "market não instalado\n{t}"
    );
    let calls = b.log("calls.log");
    assert!(
        calls.contains("schematize-market-freebsd-x86_64 install --all -y"),
        "kit sem market --all -y:\n{calls}"
    );
    assert!(usos_de_escalador(&b).is_empty(), "sudo chamado à toa: {:?}", usos_de_escalador(&b));
    assert!(!t.contains("só Linux"), "morreu como não-Linux:\n{t}");
}

/// Contraparte do hermético: o teste acima só prova "sem sudo à toa" se, COM uma cópia antiga no
/// diretório de sistema, o script de fato escala. Sem este, esvaziar o dir esconderia a regressão.
#[test]
fn copia_antiga_no_dir_de_sistema_ainda_escala_para_remover() {
    let b = Box::novo("copia-antiga");
    b.uname("FreeBSD", "amd64");
    b.baixador("fetch");
    b.escalador("sudo");
    b.falso("pkg", "exit 0");
    fs::write(b.sys().join("schematize"), "velho").unwrap();
    let _ = b.instala(&["--binary"]);
    let usos = usos_de_escalador(&b);
    assert!(
        usos.iter().any(|l| l.contains("rm -f") && l.contains("sys/schematize")),
        "devia remover a cópia antiga com sudo: {usos:?}"
    );
}

#[test]
fn freebsd_x86_64_no_uname_tambem_mapeia() {
    let b = Box::novo("x8664");
    b.uname("FreeBSD", "x86_64");
    b.baixador("fetch");
    b.falso("pkg", "exit 0");
    let _ = b.instala(&["--binary"]);
    assert!(b.log("fetch.log").contains("schematize-freebsd-x86_64"), "{}", b.log("fetch.log"));
}

#[test]
fn freebsd_arch_sem_binario_da_mensagem_acionavel_sem_baixar_asset() {
    let b = Box::novo("arm64");
    b.uname("FreeBSD", "arm64");
    b.baixador("fetch");
    b.falso("pkg", "exit 0");
    let o = b.instala(&["--binary"]);
    let t = texto(&o);
    assert_eq!(o.status.code(), Some(2), "{t}");
    assert!(
        t.contains("FreeBSD/arm64") && t.contains("--from-source"),
        "mensagem sem caminho:\n{t}"
    );
    assert!(
        !b.log("fetch.log").contains("schematize-freebsd"),
        "não devia pedir asset:\n{}",
        b.log("fetch.log")
    );
}

#[test]
fn regressao_linux_usa_curl_e_assets_linux() {
    let b = Box::novo("linux");
    b.uname("Linux", "x86_64");
    b.baixador("curl");
    b.escalador("sudo");
    let o = b.instala(&["--binary"]);
    let t = texto(&o);
    let f = b.log("fetch.log");
    for esperado in [
        "/releases/latest/download/schematize-linux-x86_64",
        "/releases/latest/download/schematize-gui-linux-x86_64",
        "/schematize_market_rs/releases/latest/download/schematize-market-linux-x86_64",
    ] {
        assert!(f.contains(esperado), "faltou {esperado}\nfetch.log:\n{f}\nsaida:\n{t}");
    }
    assert!(!f.contains("freebsd"), "Linux pediu asset freebsd:\n{f}");
    assert!(b.raiz.join("home/.cargo/bin/schematize").is_file(), "hub não instalado\n{t}");
}

/// Pedaço do `install.sh` que o `sh` puro precisa entender: do `if [ -z "${BASH_VERSION..."` ao `fi`
/// de coluna 0. Substitui o NOME da variável (o /bin/sh deste host pode ser bash, que a define).
fn preambulo() -> String {
    let src = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh")).unwrap();
    let ini = src.find("if [ -z \"${BASH_VERSION:-}\" ]; then").expect("sem preambulo POSIX");
    let resto = &src[ini..];
    let fim = resto.find("\nfi\n").expect("preambulo sem fi") + 4;
    resto[..fim].replace("BASH_VERSION", "SCHEMATIZE_TESTE_NAO_E_BASH")
}

#[test]
fn preambulo_posix_instala_bash_por_pkg_via_escalador_e_reexecuta() {
    let b = Box::novo("preambulo");
    b.uname("FreeBSD", "amd64");
    b.falso("id", "echo 1000");
    b.escalador("doas");
    // `pkg install -y bash` falso: "instala" criando o bash no PATH.
    b.falso(
        "pkg",
        &format!(
            "echo \"pkg $*\" >> \"$LOGDIR/pkg.log\"\nln -sf /bin/bash \"{}/bash\"",
            b.bin().display()
        ),
    );
    // `fetch` falso: serve um install.sh que só ecoa os argumentos (prova o re-exec sob bash).
    b.falso(
        "fetch",
        "echo \"$*\" >> \"$LOGDIR/fetch.log\"\nfor a in \"$@\"; do o=\"$a\"; done\nprev=\"\"; for a in \"$@\"; do [ \"$prev\" = -o ] && out=\"$a\"; prev=\"$a\"; done\nprintf '#!/usr/bin/env bash\\necho REEXEC \"$@\"\\n' > \"$out\"",
    );
    let p = b.raiz.join("preambulo.sh");
    fs::write(&p, preambulo()).unwrap();
    // Shell POSIX ESTRITO quando existir (dash/busybox/posh/mksh — a CI ubuntu tem dash): o
    // /bin/sh de alguns hosts é bash e aceitaria bash-ismo no preâmbulo sem reclamar.
    let estrito = ["/usr/bin/dash", "/bin/dash", "/usr/bin/posh", "/usr/bin/mksh"]
        .iter()
        .find(|c| Path::new(c).exists())
        .copied()
        .unwrap_or("/bin/sh");
    eprintln!("preambulo testado sob: {estrito}");
    if std::env::var_os("CI").is_some() {
        assert_ne!(estrito, "/bin/sh", "CI sem shell POSIX estrito (dash) — instale-o no runner");
    }
    let o = Command::new(estrito)
        .arg("-c")
        .arg(format!(". '{}'", p.display()))
        .arg("sh") // $0 = "sh": o caso do `... | sh -s -- --completo`
        .arg("--completo")
        .env_clear()
        .env("PATH", b.bin())
        .env("LOGDIR", b.raiz.join("log"))
        .output()
        .unwrap();
    let t = texto(&o);
    assert!(b.log("pkg.log").contains("install -y bash"), "não instalou o bash:\n{t}");
    assert!(
        b.log("sudo.log").contains("doas pkg install -y bash"),
        "não escalou por doas:\n{}",
        b.log("sudo.log")
    );
    assert!(b
        .log("fetch.log")
        .contains("raw.githubusercontent.com/schematizeme/schematize-cli/main/install.sh"));
    assert!(t.contains("REEXEC --completo"), "não re-executou sob bash:\n{t}");
}

#[test]
fn preambulo_e_noop_sob_bash() {
    // Sob bash `BASH_VERSION` existe: o bloco inteiro é pulado e o script segue (aqui, até o `die`
    // do sistema não suportado, que prova que passou do preâmbulo sem exec).
    let b = Box::novo("noop");
    b.uname("Plan9", "x86_64");
    b.falso("curl", "exit 22");
    let o = b.instala(&["--binary"]);
    assert!(texto(&o).contains("Linux e FreeBSD"), "{}", texto(&o));
}
