//! Hooks do overdev sem o app instalado: o veto NÃO pode sumir em silêncio.
//!
//! **O quê:** roda o binário de verdade com HOME e PATH descartáveis e confere código de
//! saída, stdout (JSON de bloqueio) e stderr de `overdev check` / `overdev guard` quando o
//! `schematize-overdev` está ausente — e que, com ele presente, a delegação não muda.
//!
//! **Onde:** guarda da ponte `overdevausente` (ADR-0012 F5). O Claude Code trata rc != 0/2 de
//! hook como erro NÃO bloqueante; o `rc=1` antigo desligava o veto de um run ativo.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Resultado de uma invocação: (rc, stdout, stderr).
type Saida = (i32, String, String);

/// **O quê:** cria um sandbox descartável (HOME + projeto) sob o alvo do cargo.
/// **Onde:** início de cada teste.
fn sandbox(nome: &str) -> PathBuf {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("teste-overdev-ausente")
        .join(format!("{}-{nome}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    std::fs::create_dir_all(raiz.join("home")).expect("cria HOME");
    std::fs::create_dir_all(raiz.join("proj")).expect("cria projeto");
    raiz
}

/// **O quê:** grava `.schematize/overdev/state.json` com o `mode` dado no projeto.
/// **Onde:** testes que precisam de run ativo/encerrado.
fn grava_run(proj: &Path, mode: &str) {
    let d = proj.join(".schematize/overdev");
    std::fs::create_dir_all(&d).expect("cria dir do overdev");
    let json = format!(r#"{{"mode":"{mode}","max_iters":200,"objetivo":"x","started":0}}"#);
    std::fs::write(d.join("state.json"), json).expect("grava state.json");
}

/// **O quê:** roda `schematize overdev <sub>` no projeto, com `stdin` dado, HOME temporário e
/// `path` como PATH. **Onde:** todos os testes.
fn roda(raiz: &Path, sub: &str, stdin: &str, path: &str) -> Saida {
    use std::io::Write;
    let mut filho = Command::new(env!("CARGO_BIN_EXE_schematize"))
        .args(["overdev", sub])
        .current_dir(raiz.join("proj"))
        .env("HOME", raiz.join("home"))
        .env("XDG_CONFIG_HOME", raiz.join("home/.config"))
        .env("XDG_DATA_HOME", raiz.join("home/.local/share"))
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("executa o binário");
    filho.stdin.take().expect("stdin").write_all(stdin.as_bytes()).expect("escreve stdin");
    let o = filho.wait_with_output().expect("espera");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// App ausente + run ativo, `check`: rc 0 e o JSON de bloqueio que o Claude Code entende.
#[test]
fn check_sem_app_e_run_ativo_bloqueia_com_json() {
    let r = sandbox("check-ativo");
    grava_run(&r.join("proj"), "active");
    let (rc, out, err) = roda(&r, "check", "{}", "/usr/bin:/bin");
    assert_eq!(rc, 0, "rc != 0 desliga o veto no Claude Code; stderr={err}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect("stdout é JSON");
    assert_eq!(v["decision"], "block", "{out}");
    let motivo = v["reason"].as_str().expect("reason");
    assert!(motivo.contains("schematize-market install schematize-overdev"), "{motivo}");
    assert!(motivo.contains("schematize overdev stop"), "{motivo}");
    assert!(motivo.contains("ATIVO"), "{motivo}");
}

/// App ausente + run encerrado (`mode != active`): não é run ativo, não bloqueia.
#[test]
fn check_sem_app_e_run_encerrado_nao_bloqueia() {
    let r = sandbox("check-done");
    grava_run(&r.join("proj"), "done");
    let (rc, out, _) = roda(&r, "check", "{}", "/usr/bin:/bin");
    assert_eq!(rc, 0);
    assert!(out.trim().is_empty(), "stdout deve ser vazio: {out}");
}

/// App ausente + sem run: rc 0, stdout vazio, dica curta no stderr.
#[test]
fn check_sem_app_e_sem_run_e_silencioso_no_stdout() {
    let r = sandbox("check-sem-run");
    let (rc, out, err) = roda(&r, "check", "{}", "/usr/bin:/bin");
    assert_eq!(rc, 0, "{err}");
    assert!(out.trim().is_empty(), "stdout deve ser vazio: {out}");
    assert!(err.contains("schematize-market install schematize-overdev"), "{err}");
}

/// App ausente, `state.json` corrompido: não é run ativo comprovado, mas também não derruba.
#[test]
fn check_sem_app_e_state_corrompido_nao_quebra() {
    let r = sandbox("check-lixo");
    let d = r.join("proj/.schematize/overdev");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("state.json"), "{ nao e json").unwrap();
    let (rc, out, _) = roda(&r, "check", "{}", "/usr/bin:/bin");
    assert_eq!(rc, 0);
    assert!(out.trim().is_empty(), "{out}");
}

/// `stop_hook_active=true` com o app ainda ausente: já avisamos uma vez; bloquear de novo
/// prenderia o agente num laço sem saída. Libera (rc 0, sem JSON) e avisa no stderr.
#[test]
fn check_sem_app_respeita_stop_hook_active() {
    let r = sandbox("check-sha");
    grava_run(&r.join("proj"), "active");
    let (rc, out, err) = roda(&r, "check", r#"{"stop_hook_active":true}"#, "/usr/bin:/bin");
    assert_eq!(rc, 0);
    assert!(out.trim().is_empty(), "não pode re-bloquear em laço: {out}");
    assert!(err.contains("schematize-market install schematize-overdev"), "{err}");
}

/// App ausente, `guard`, run ativo: NÃO bloqueia (bloquear travaria o agente antes de ele
/// poder instalar o app). rc 0, sem stdout, aviso no stderr.
#[test]
fn guard_sem_app_nao_bloqueia_mas_avisa() {
    let r = sandbox("guard-ativo");
    grava_run(&r.join("proj"), "active");
    let (rc, out, err) = roda(&r, "guard", "{}", "/usr/bin:/bin");
    assert_eq!(rc, 0, "{err}");
    assert!(out.trim().is_empty(), "guard não pode bloquear sem o app: {out}");
    assert!(err.contains("schematize-market install schematize-overdev"), "{err}");
}

/// App ausente, `guard`, sem run: rc 0 e nada no stdout.
#[test]
fn guard_sem_app_e_sem_run_e_silencioso() {
    let r = sandbox("guard-sem-run");
    let (rc, out, _) = roda(&r, "guard", "{}", "/usr/bin:/bin");
    assert_eq!(rc, 0);
    assert!(out.trim().is_empty(), "{out}");
}

/// App PRESENTE: a delegação não muda — o rc e a saída são os do app, mesmo com run ativo.
#[test]
fn com_app_presente_a_delegacao_segue_igual() {
    use std::os::unix::fs::PermissionsExt;
    let r = sandbox("presente");
    grava_run(&r.join("proj"), "active");
    let bin = r.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let falso = bin.join("schematize-overdev");
    std::fs::write(&falso, "#!/bin/sh\necho \"FAKE-APP $*\"\nexit 7\n").unwrap();
    std::fs::set_permissions(&falso, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let (rc, out, _) = roda(&r, "check", "{}", &path);
    assert_eq!(rc, 7, "rc do app tem de passar cru");
    assert_eq!(out.trim(), "FAKE-APP check");
    let (rc, out, _) = roda(&r, "guard", "{}", &path);
    assert_eq!(rc, 7);
    assert_eq!(out.trim(), "FAKE-APP guard");
}
