//! O QUE: prova que o `install.sh` PADRÃO (sem flags) instala a skill `engineering` e aplica o
//! núcleo sempre-on (`schematize core install`), que `--sem-nucleo` desliga isso, e que falha de
//! rede / ausência de `~/.claude` nunca derrubam a instalação do app.
//!
//! POR QUE: a decisão do usuário é que máquina recém-formatada saia com a regra de orquestração
//! ativa. Antes só `--skills`/`--completo` faziam isso, e o instalador padrão deixava o agente sem
//! a regra no contexto.
//!
//! COMO: roda o `install.sh` REAL num sandbox (HOME falso, PATH só com symlinks mínimos, `curl`
//! falso que serve binários falsos que registram as chamadas). Uma prova de ponta a ponta usa o
//! binário `schematize` de verdade, só com a skill falsa, e lê o `CLAUDE.md` do HOME falso. Nada
//! toca a rede nem o `~/.claude` real.
#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const REAIS: &[&str] = &[
    "env", "id", "mkdir", "mktemp", "rm", "cp", "mv", "chmod", "install", "cat", "grep", "sed",
    "head", "tail", "tr", "awk", "sort", "cut", "basename", "dirname", "date", "touch", "ls",
    "getent", "logname", "tty", "stat", "pkill", "ln", "wc", "sleep", "sh", "readlink", "uniq",
    "find", "xargs", "dpkg", "rpm", "tee", "true",
];

struct Box {
    raiz: PathBuf,
}

impl Box {
    /// `skills_ok`: o `schematize-skills install engineering` falso funciona? `hub_real`: serve o
    /// binário `schematize` de verdade em vez do falso.
    fn novo(nome: &str, skills_ok: bool, hub_real: bool) -> Box {
        let raiz =
            std::env::temp_dir().join(format!("schematize-nucleo-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        for d in ["bin", "home", "log", "sys"] {
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
        b.uname();
        b.curl(skills_ok, hub_real);
        b
    }
    fn falso(&self, nome: &str, corpo: &str) {
        let p = self.raiz.join("bin").join(nome);
        let _ = fs::remove_file(&p);
        fs::write(&p, format!("#!/bin/sh\n{corpo}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn uname(&self) {
        self.falso(
            "uname",
            "case \"$1\" in -s) echo Linux;; -m) echo x86_64;; -r) echo 6.1;; *) echo Linux;; esac",
        );
    }
    /// `curl` falso: serve, no `-o`, um executável conforme o asset pedido.
    fn curl(&self, skills_ok: bool, hub_real: bool) {
        let real = env!("CARGO_BIN_EXE_schematize");
        let skills_rc = if skills_ok { 0 } else { 1 };
        // Conteúdo servido para cada asset. Os falsos registram "<nome> <args>" em calls.log.
        let corpo = format!(
            r#"url=""; out=""; prev=""
for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; url="$a"; done
echo "$url" >> "$LOGDIR/fetch.log"
case "$url" in *api.github.com*) exit 22;; esac
[ -z "$out" ] && exit 0
[ "$out" = "-" ] && exit 22
[ "$out" = "/dev/null" ] && exit 0
base="${{url##*/}}"
case "$base" in
  schematize-market-*)
    printf '#!/bin/sh\necho "market $*" >> "%s/calls.log"\ncase "$1" in --version) echo "schematize-market 9.9.9";; install) case "$2" in schematize-skills) cp "%s/bin/skills-modelo" "$HOME/.cargo/bin/schematize-skills";; esac;; esac\nexit 0\n' "$LOGDIR" "$RAIZ" > "$out";;
  schematize-gui-*)
    printf '#!/bin/sh\ncase "$1" in --version) echo "schematize-gui 9.9.9";; esac\nexit 0\n' > "$out";;
  schematize-*)
    if [ "{hub_real}" = true ]; then cp "{real}" "$out"; else
    printf '#!/bin/sh\necho "schematize $*" >> "%s/calls.log"\ncase "$1" in --version) echo "schematize 9.9.9";; esac\nexit 0\n' "$LOGDIR" > "$out"; fi;;
  *) printf '#!/bin/sh\nexit 0\n' > "$out";;
esac
chmod +x "$out""#
        );
        self.falso("curl", &corpo);
        // Modelo do `schematize-skills` falso: instala a "engineering" (CORE.md mínimo) ou falha.
        let skills = format!(
            "echo \"schematize-skills $*\" >> \"$LOGDIR/calls.log\"\ncase \"$1 $2\" in \"install engineering\")\n  [ {skills_rc} -ne 0 ] && {{ echo 'sem rede' >&2; exit 1; }}\n  d=\"$HOME/.claude/skills/schematize-engineering/assets\"; mkdir -p \"$d\"\n  printf '<!-- core-version: 0.99.0 -->\\n# Núcleo falso\\nOrquestre com subagents.\\n' > \"$d/CORE.md\";;\nesac\nexit 0"
        );
        self.falso("skills-modelo", &skills);
    }
    fn log(&self, nome: &str) -> String {
        fs::read_to_string(self.raiz.join("log").join(nome)).unwrap_or_default()
    }
    fn home(&self) -> PathBuf {
        self.raiz.join("home")
    }
    fn instala(&self, args: &[&str]) -> Output {
        let sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
        Command::new("/bin/bash")
            .arg(sh)
            .args(args)
            .env_clear()
            .env("PATH", self.raiz.join("bin"))
            .env("HOME", self.home())
            .env("LOGDIR", self.raiz.join("log"))
            .env("RAIZ", &self.raiz)
            .env("SCHEMATIZE_SYS_BIN_DIRS", self.raiz.join("sys"))
            .env("SCHEMATIZE_INSTALL_NO_SELF", "1")
            .output()
            .expect("bash roda")
    }
}

fn texto(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn padrao_sem_flags_instala_engineering_e_aplica_o_nucleo() {
    let b = Box::novo("padrao", true, false);
    let o = b.instala(&["--binary"]);
    let (t, calls) = (texto(&o), b.log("calls.log"));
    assert!(o.status.success(), "{t}");
    assert!(
        calls.contains("schematize-skills install engineering"),
        "sem engineering:\n{calls}\n{t}"
    );
    assert!(calls.contains("schematize core install"), "sem core install:\n{calls}\n{t}");
    // engineering ANTES do núcleo: o core install lê o CORE.md que a skill traz.
    let (i, c) = (calls.find("install engineering").unwrap(), calls.find("core install").unwrap());
    assert!(i < c, "ordem errada:\n{calls}");
}

#[test]
fn sem_nucleo_nao_instala_engineering_nem_aplica_o_nucleo() {
    let b = Box::novo("optout", true, false);
    let o = b.instala(&["--binary", "--sem-nucleo"]);
    let (t, calls) = (texto(&o), b.log("calls.log"));
    assert!(o.status.success(), "{t}");
    assert!(!calls.contains("install engineering"), "instalou engineering:\n{calls}");
    assert!(!calls.contains("core install"), "aplicou o núcleo:\n{calls}");
    assert!(b.home().join(".cargo/bin/schematize").is_file(), "o app devia estar instalado\n{t}");
    assert!(!b.home().join(".claude").exists(), "não devia mexer em ~/.claude");
}

#[test]
fn falha_de_rede_na_skill_nao_derruba_o_app_e_diz_o_comando() {
    let b = Box::novo("rede", false, false);
    let o = b.instala(&["--binary"]);
    let t = texto(&o);
    assert!(o.status.success(), "a falha da skill derrubou a instalação:\n{t}");
    assert!(b.home().join(".cargo/bin/schematize").is_file(), "app não instalado\n{t}");
    assert!(
        t.contains("schematize skills install engineering"),
        "sem o comando pra completar:\n{t}"
    );
}

#[test]
fn sem_dot_claude_cria_e_avisa_sem_falhar() {
    let b = Box::novo("semclaude", true, false);
    assert!(!b.home().join(".claude").exists());
    let o = b.instala(&["--binary"]);
    let t = texto(&o);
    assert!(o.status.success(), "{t}");
    assert!(b.home().join(".claude").is_dir(), "devia preparar ~/.claude\n{t}");
    assert!(t.contains("Claude Code ainda nao esta instalado"), "sem o aviso claro:\n{t}");
    assert!(
        !t.to_lowercase().contains("voce deve") && !t.contains("você deve"),
        "culpou o usuário:\n{t}"
    );
}

/// Ponta a ponta com o `schematize` REAL: o bloco do núcleo tem de aparecer no CLAUDE.md do HOME
/// falso e o hook no settings.json. Só a skill (rede) é falsa.
#[test]
fn ponta_a_ponta_grava_o_bloco_no_claude_md_do_home_falso() {
    let b = Box::novo("e2e", true, true);
    let o = b.instala(&["--binary"]);
    let t = texto(&o);
    assert!(o.status.success(), "{t}");
    let cm = fs::read_to_string(b.home().join(".claude/CLAUDE.md")).unwrap_or_default();
    assert!(cm.contains("<!-- schematize:core:begin -->"), "sem o bloco:\n{cm}\n{t}");
    assert!(cm.contains("Orquestre com subagents."), "bloco sem o conteúdo do CORE.md:\n{cm}");
    let st = fs::read_to_string(b.home().join(".claude/settings.json")).unwrap_or_default();
    assert!(st.contains("core print"), "sem o hook SessionStart:\n{st}");
    if std::env::var_os("MOSTRAR_PROVA").is_some() {
        eprintln!("--- {}/.claude/CLAUDE.md ---\n{cm}", b.home().display());
    }
}
