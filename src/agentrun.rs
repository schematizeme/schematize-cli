//! Execução do overdev com um agente de código ACOPLADO (Fase 4 do redesenho).
//! O quê: spawna o CLI do agente (`claude`, default) num PTY dentro do projeto,
//! liga o overdev e MONITORA — imprime o output, lê o progresso do `.overdev/`
//! (via `overdev::progress_at`) e, se o agente ficar OCIOSO com itens `- [ ]`
//! abertos, injeta `continue` + a lista dos itens pra ele retomar sozinho.
//! Onde: CLI `schematize overdev run` (síncrono, `run_attached`); a GUI reusa a
//! mesma `Session` (Receiver de output + `send` de input + `overdev::progress_at`).
//!
//! ## API que a GUI vai consumir
//! - [`AgentRunner`] — trait plugável; [`ClaudeRunner`] é o default. `spawn()`
//!   devolve uma [`Session`].
//! - [`Session`] — handle do processo acoplado, `Send` (pode viver numa thread da
//!   GUI): [`Session::recv_timeout`]/[`Session::try_recv`] drenam o output do PTY,
//!   [`Session::send`] injeta input (ex.: `continue\n`), [`Session::is_alive`] diz
//!   se o agente ainda roda e [`Session::kill`] encerra.
//! - Progresso ao vivo: `overdev::progress_at(project)` (barra/contagem) e
//!   `overdev::open_items_at(project, n)` (itens abertos pra a lista do nudge).
//! - Peças PURAS reusáveis/testáveis: [`should_nudge`] e [`nudge_message`].
//!
//! ## Guardrails
//! O comando do agente é MOSTRADO antes de disparar (o confirm fica no `main`,
//! pra a GUI ter o seu próprio); `run_attached` respeita `max` (teto de nudges,
//! nunca injeta `continue` infinito) e NUNCA injeta segredo — só o `continue` +
//! a lista de itens lida do próprio `.overdev/CHECKLIST.md`.

use crate::overdev;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Segundos sem output novo do PTY que caracterizam OCIOSIDADE do agente.
/// Acima disso, com item `- [ ]` aberto, `run_attached` injeta o `continue`.
pub const IDLE_THRESHOLD_SECS: u64 = 45;

/// Teto default de nudges (`continue`) quando o CLI não recebe `--max`.
pub const DEFAULT_MAX_NUDGES: u64 = 20;

/// Quantos itens abertos, no máximo, entram na mensagem de nudge.
const NUDGE_ITEMS: usize = 8;

// ---------------------------------------------------------------------------
// Peças PURAS (sem PTY) — testáveis e reusadas pela GUI.
// ---------------------------------------------------------------------------

/// Decide se é hora de cutucar o agente: ocioso há `idle_secs` E ainda há itens
/// de máquina abertos. PURA — a GUI e os testes chamam sem spawnar nada.
pub fn should_nudge(idle_secs: u64, open_items: usize) -> bool {
    idle_secs >= IDLE_THRESHOLD_SECS && open_items > 0
}

/// Monta a mensagem injetada no PTY quando o agente pausa: SEMPRE começa por
/// `continue` (retoma o loop do overdev) e, se houver itens, anexa a lista pra
/// focar o trabalho. Termina em `\n` pra o agente SUBMETER a linha. PURA.
/// Só usa texto do próprio `.overdev/` — nunca segredo.
pub fn nudge_message(open_items: &[String]) -> String {
    let mut s = String::from("continue");
    if !open_items.is_empty() {
        s.push_str("\nNÃO PARE — ainda há itens de máquina abertos no .overdev/CHECKLIST.md. Revise e feche estes:");
        for it in open_items {
            s.push('\n');
            s.push_str(it);
        }
    }
    s.push('\n');
    s
}

/// Prompt inicial passado como ARGUMENTO do `claude` (o claude interativo o submete sozinho).
/// É linguagem NATURAL — não o slash `/eng-overdev` (que não dispara como arg). Dá o método do
/// overdev direto e conta com o Stop hook pra impor o "não pare". PURA.
pub fn overdev_prompt(objetivo: &str) -> String {
    let o = objetivo.trim();
    let alvo = if o.is_empty() {
        String::new()
    } else {
        format!(" O objetivo é: {o}.")
    };
    format!(
        "Modo OVERDEV neste projeto.{alvo} Leia `.overdev/CHECKLIST.md` (e `.overdev/OBJETIVO.md`, \
         `.overdev/PLAN.md`, `.overdev/DECISOES.md` se existirem) e TRABALHE cada item aberto \
         `- [ ]` até fechar, marcando `- [x]` só COM PROVA (teste/comando/arquivo/gate que passa). \
         Itens `- [H ]` são de humano — não os faça. NÃO pare enquanto houver `- [ ]` aberto (o Stop \
         hook do overdev vai te barrar de qualquer forma). Se travar numa dúvida, use \
         `schematize overdev park` e siga. Comece agora pelo primeiro item aberto."
    )
}

/// `true` se o CLL `claude` está no `$PATH` (checagem barata pra a GUI/CLI
/// decidirem entre disparar a sessão ou só imprimir a dica).
pub fn claude_in_path() -> bool {
    binary_in_path("claude")
}

/// `true` se `bin` é um arquivo em algum diretório do `$PATH` (checagem barata
/// pra dar erro claro antes de tentar spawnar um agente inexistente).
fn binary_in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

/// Marca `projects.<caminho>.hasTrustDialogAccepted = true` no `~/.claude.json` pra o
/// `claude` NÃO exibir o "Is this a project you trust?" (que trava o run acoplado). Best-effort:
/// qualquer erro é ignorado (no pior caso o agente só mostra o prompt, como antes). Preserva todo
/// o resto do config (merge por campo). Não é campo de segurança do nosso lado — o usuário
/// disparou o overdev no próprio projeto.
fn pre_trust_project(project: &Path) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let cfg = Path::new(&home).join(".claude.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&cfg)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let Some(obj) = root.as_object_mut() else { return };
    let key = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf())
        .to_string_lossy()
        .to_string();
    let projects = obj
        .entry("projects")
        .or_insert_with(|| serde_json::json!({}));
    let Some(pobj) = projects.as_object_mut() else { return };
    let entry = pobj.entry(key).or_insert_with(|| serde_json::json!({}));
    if let Some(eo) = entry.as_object_mut() {
        eo.insert("hasTrustDialogAccepted".into(), serde_json::Value::Bool(true));
        let _ = std::fs::write(&cfg, serde_json::to_string_pretty(&root).unwrap_or_default());
    }
}

/// Dispara o overdev num TERMINAL EXTERNO (processo próprio do `claude`, RAM dele — NÃO carrega o
/// load dentro do app; some o inchaço tipo VSCode). O app só MONITORA o `.overdev/` depois. O
/// "não pare" é imposto pelo Stop hook do overdev (não precisa injetar `continue`).
///
/// Escreve o prompt num arquivo (evita quoting de shell) + um wrapper `.overdev/run.sh`, pré-confia
/// a pasta e abre o 1º terminal disponível rodando `claude --dangerously-skip-permissions "<prompt>"`.
/// Devolve o nome do terminal usado. Erro se `claude` ou nenhum terminal estiver no PATH.
pub fn launch_in_terminal(project: &Path, objetivo: &str) -> Result<String, String> {
    if !binary_in_path("claude") {
        return Err("o CLI `claude` não está no PATH — instale o Claude Code (claude.ai/code).".into());
    }
    pre_trust_project(project);
    let od = project.join(".overdev");
    std::fs::create_dir_all(&od).map_err(|e| format!("criar .overdev: {e}"))?;
    let promptfile = od.join("run-prompt.txt");
    std::fs::write(&promptfile, overdev_prompt(objetivo)).map_err(|e| format!("gravar prompt: {e}"))?;
    let script = od.join("run.sh");
    let sh = format!(
        "#!/usr/bin/env bash\ncd {proj:?} || exit 1\nclaude --dangerously-skip-permissions \"$(cat {pf:?})\"\necho\nread -rp '[overdev encerrado — Enter para fechar] '\n",
        proj = project,
        pf = promptfile,
    );
    std::fs::write(&script, &sh).map_err(|e| format!("gravar wrapper: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
    }
    let terms = [
        "konsole",
        "gnome-terminal",
        "xfce4-terminal",
        "x-terminal-emulator",
        "alacritty",
        "kitty",
        "xterm",
    ];
    let term = terms
        .iter()
        .find(|t| binary_in_path(t))
        .ok_or_else(|| "nenhum terminal encontrado (konsole/gnome-terminal/xterm/…).".to_string())?;
    let mut cmd = std::process::Command::new(term);
    match *term {
        // esses usam `--` pra separar o comando a executar
        "gnome-terminal" | "xfce4-terminal" => {
            cmd.arg("--").arg("bash").arg(&script);
        }
        _ => {
            cmd.arg("-e").arg("bash").arg(&script);
        }
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // desacopla do app: o claude segue vivo se o app fechar
    }
    cmd.spawn().map_err(|e| format!("abrir terminal `{term}`: {e}"))?;
    Ok((*term).to_string())
}

// ---------------------------------------------------------------------------
// Trait plugável + implementação default (Claude Code CLI em PTY).
// ---------------------------------------------------------------------------

/// Agente de código plugável. Default = [`ClaudeRunner`]; futuros (GPT, etc.)
/// só implementam este trait sem tocar no `run_attached`.
pub trait AgentRunner {
    /// Sobe o agente no `project` (num PTY) com o `objetivo`, já ligando o
    /// overdev. Devolve a [`Session`] acoplada (output + input + status).
    fn spawn(&self, project: &Path, objetivo: &str) -> Result<Session, String>;

    /// Comando humano-legível MOSTRADO no guardrail antes de disparar.
    fn command_line(&self, objetivo: &str) -> String;

    /// Nome curto do agente (mensagens/log).
    fn name(&self) -> &str {
        "agente"
    }
}

/// Default: o CLI `claude` (Claude Code) num pseudo-terminal. Todo o overdev
/// (hooks Stop/PreToolUse) já é feito pra ele.
pub struct ClaudeRunner;

impl AgentRunner for ClaudeRunner {
    fn name(&self) -> &str {
        "claude"
    }

    fn command_line(&self, objetivo: &str) -> String {
        let _ = objetivo;
        "claude --dangerously-skip-permissions \"<prompt do overdev>\"  (cwd = projeto)".to_string()
    }

    fn spawn(&self, project: &Path, objetivo: &str) -> Result<Session, String> {
        // Pré-confia a pasta no ~/.claude.json (senão o claude para na tela "Is this a project
        // you trust?" — que o `--dangerously-skip-permissions` NÃO pula, e o auto-continue não
        // responde). O usuário disparou o overdev no próprio projeto, então confiar é implícito.
        pre_trust_project(project);
        if !binary_in_path("claude") {
            return Err(
                "o CLI `claude` não está no PATH. Instale o Claude Code (claude.ai/code) \
                 e garanta que `claude` roda no terminal antes de usar `overdev run`."
                    .to_string(),
            );
        }
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("falha ao abrir PTY: {e}"))?;

        let mut cmd = CommandBuilder::new("claude");
        // `--dangerously-skip-permissions`: pula o "trust this folder?" e os prompts de
        // permissão de ferramenta — o overdev roda AUTÔNOMO (o usuário disparou no próprio
        // projeto). Sem isso o agente trava na tela de confiança (o auto-continue não a responde).
        cmd.arg("--dangerously-skip-permissions");
        // O prompt inicial vai como ARGUMENTO posicional (o claude interativo o submete sozinho).
        // Digitar no PTY não funciona (bracketed paste). Comando natural, NÃO o slash `/eng-overdev`
        // (que não dispara como arg). O laço "não pare" é imposto pelo Stop hook do overdev.
        cmd.arg(overdev_prompt(objetivo));
        cmd.cwd(project);
        // Ambiente ROBUSTO: quando a GUI é aberta pelo lançador do desktop, o env é mínimo e o
        // `claude` (node) morre na hora (virava zumbi). Herda o env do pai e reforça os
        // essenciais — HOME, TERM e um PATH com os bins de usuário (~/.local/bin, ~/.cargo/bin).
        for (k, v) in std::env::vars() {
            cmd.env(k, v);
        }
        if std::env::var_os("TERM").is_none() {
            cmd.env("TERM", "xterm-256color");
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = home.to_string_lossy().to_string();
            let base = std::env::var("PATH").unwrap_or_default();
            let mut parts: Vec<String> =
                base.split(':').filter(|s| !s.is_empty()).map(String::from).collect();
            for e in [
                format!("{home}/.local/bin"),
                format!("{home}/.cargo/bin"),
                "/usr/local/bin".to_string(),
                "/usr/bin".to_string(),
                "/bin".to_string(),
            ] {
                if !parts.contains(&e) {
                    parts.push(e);
                }
            }
            cmd.env("PATH", parts.join(":"));
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("falha ao spawnar `claude`: {e}"))?;
        // Fecha o lado slave no processo pai: assim o reader recebe EOF quando o
        // agente termina (base pra `is_alive`/fim do loop).
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|e| format!("PTY reader: {e}"))?;
        let writer = pair.master.take_writer().map_err(|e| format!("PTY writer: {e}"))?;

        // Thread de leitura: empurra pedaços de output do PTY pro canal.
        let (tx, rx) = mpsc::channel::<String>();
        let reader_thread = thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF: o agente saiu
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                        if tx.send(chunk).is_err() {
                            break; // consumidor foi embora
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let session = Session {
            rx,
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            _master: pair.master,
            _reader_thread: Some(reader_thread),
        };
        // O comando inicial já foi passado como ARGUMENTO (submete sozinho) — nada a digitar aqui.
        Ok(session)
    }
}

// ---------------------------------------------------------------------------
// Session — handle do processo acoplado (desenhada pra o CLI E a GUI).
// ---------------------------------------------------------------------------

/// Handle do agente acoplado. `Send`: pode ser movida pra uma thread de trabalho
/// da GUI. Consumo: drene [`recv_timeout`](Session::recv_timeout)/
/// [`try_recv`](Session::try_recv) pro widget de log, chame
/// [`send`](Session::send) pra injetar input, e cheque
/// [`is_alive`](Session::is_alive) / `overdev::progress_at` pra o progresso.
pub struct Session {
    rx: Receiver<String>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    // Mantém o master vivo enquanto a sessão existe (segura os FDs do writer/reader).
    _master: Box<dyn MasterPty + Send>,
    // Handle da thread de leitura (só pra manter posse; encerra sozinha no EOF).
    _reader_thread: Option<JoinHandle<()>>,
}

impl Session {
    /// Injeta `s` no stdin do agente (via PTY). Inclua o `\n` pra ele submeter.
    pub fn send(&self, s: &str) -> Result<(), String> {
        let mut w = self.writer.lock().map_err(|_| "writer envenenado".to_string())?;
        w.write_all(s.as_bytes()).map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())
    }

    /// Pega o próximo pedaço de output sem bloquear (None se nada agora).
    pub fn try_recv(&self) -> Option<String> {
        self.rx.try_recv().ok()
    }

    /// Espera até `d` por output (None no timeout ou se o canal fechou).
    pub fn recv_timeout(&self, d: Duration) -> Option<String> {
        self.rx.recv_timeout(d).ok()
    }

    /// `true` enquanto o agente ainda roda (false após sair ou em erro).
    pub fn is_alive(&self) -> bool {
        match self.child.lock() {
            Ok(mut c) => matches!(c.try_wait(), Ok(None)),
            Err(_) => false,
        }
    }

    /// Encerra o agente (best-effort) — usado pelo botão Parar da GUI/cleanup.
    pub fn kill(&self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }
}

// ---------------------------------------------------------------------------
// run_attached — versão CLI, síncrona: spawn + monitor + auto-continue.
// ---------------------------------------------------------------------------

/// Sobe o agente via `runner` no `project` e MONITORA até o overdev fechar.
/// - Imprime o output do PTY no stdout do CLI.
/// - Lê `overdev::progress_at(project)` a cada ciclo (mode + contagem do CHECKLIST).
/// - Detecta ociosidade ([`should_nudge`]) e injeta [`nudge_message`] com os
///   itens abertos — até `max` vezes (respeita o budget e reporta).
/// - Termina quando `mode == "stopped"` OU não há mais itens `- [ ]`, ou se o
///   agente morrer.
pub fn run_attached(project: &Path, runner: &dyn AgentRunner, max: u64) -> Result<(), String> {
    let objetivo = overdev::objetivo_at(project).unwrap_or_default();
    println!("[schematize] disparando: {}", runner.command_line(&objetivo));
    println!("[schematize] monitorando o progresso em {}/.overdev — ocioso {IDLE_THRESHOLD_SECS}s com item aberto => injeta `continue` (até {max}x).\n", project.display());

    let session = runner.spawn(project, &objetivo)?;

    let mut last_output = Instant::now();
    let mut nudges: u64 = 0;
    let mut out = std::io::stdout();

    loop {
        // 1) Drena o output disponível (bloqueia no máx. 500ms — mantém o loop vivo).
        if let Some(chunk) = session.recv_timeout(Duration::from_millis(500)) {
            let _ = out.write_all(chunk.as_bytes());
            let _ = out.flush();
            last_output = Instant::now();
            // Esvazia o resto que já chegou, pra não acumular latência.
            while let Some(more) = session.try_recv() {
                let _ = out.write_all(more.as_bytes());
                let _ = out.flush();
            }
        }

        // 2) Lê o progresso do overdev.
        let prog = overdev::progress_at(project);

        // 3) Condições de término.
        if prog.mode == "stopped" {
            println!("\n[schematize] overdev encerrado (mode=stopped). Concluído.");
            break;
        }
        if prog.mode == "active" && prog.open == 0 {
            println!("\n[schematize] 0 itens de máquina abertos — checklist fechado. Concluído.");
            break;
        }
        if !session.is_alive() {
            // Drena o que sobrou antes de sair.
            while let Some(more) = session.try_recv() {
                let _ = out.write_all(more.as_bytes());
                let _ = out.flush();
            }
            println!("\n[schematize] o agente `{}` encerrou (open={}, mode={}).", runner.name(), prog.open, prog.mode);
            break;
        }

        // 4) Auto-continue por ociosidade.
        let idle = last_output.elapsed().as_secs();
        if should_nudge(idle, prog.open) {
            if nudges >= max {
                println!(
                    "\n[schematize] budget de auto-continue esgotado ({nudges}/{max}) com {} item(ns) ainda aberto(s). Parando o monitor — retome à mão ou aumente --max.",
                    prog.open
                );
                break;
            }
            let items = overdev::open_items_at(project, NUDGE_ITEMS);
            let msg = nudge_message(&items);
            session.send(&msg)?;
            nudges += 1;
            last_output = Instant::now(); // dá tempo do agente reagir antes do próximo nudge
            println!(
                "\n[schematize] agente ocioso {idle}s com {} item(ns) aberto(s) — injetei `continue` ({nudges}/{max}).",
                prog.open
            );
        }
    }

    print_summary(project);
    Ok(())
}

/// Resumo final: contagem + on-hold + perguntas parkeadas (lido do projeto).
fn print_summary(project: &Path) {
    let p = overdev::progress_at(project);
    println!("\n===== resumo do run =====");
    println!("feitos: {}  | abertos(máquina): {}  | on-hold: {}  | humanos abertos: {}", p.done, p.open, p.hold, p.human);
    println!("ciclos: {} / max {}", p.iterations, p.max_iters);
    let perguntas = project.join("PERGUNTAS-OVERDEV.txt");
    if let Ok(q) = std::fs::read_to_string(&perguntas) {
        let n = q.lines().filter(|l| l.starts_with('[')).count();
        if n > 0 {
            println!("perguntas parkeadas: {n} (ver {})", perguntas.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nao_cutuca_antes_do_limiar() {
        assert!(!should_nudge(IDLE_THRESHOLD_SECS - 1, 3), "abaixo do limiar não cutuca");
        assert!(!should_nudge(0, 5), "recém-ativo não cutuca");
    }

    #[test]
    fn nao_cutuca_sem_itens_abertos() {
        assert!(!should_nudge(IDLE_THRESHOLD_SECS + 100, 0), "sem `- [ ]` aberto nunca cutuca");
    }

    #[test]
    fn cutuca_ocioso_com_item_aberto() {
        assert!(should_nudge(IDLE_THRESHOLD_SECS, 1), "no limiar exato com item aberto cutuca");
        assert!(should_nudge(IDLE_THRESHOLD_SECS + 30, 4));
    }

    #[test]
    fn nudge_comeca_com_continue_e_termina_em_newline() {
        let m = nudge_message(&[]);
        assert!(m.starts_with("continue"), "sempre retoma com `continue`");
        assert!(m.ends_with('\n'), "termina em newline pra submeter");
    }

    #[test]
    fn nudge_lista_os_itens_abertos() {
        let itens = vec![
            "- [ ] lib: scaffold da skill".to_string(),
            "- [ ] CLI: schematize skills new".to_string(),
        ];
        let m = nudge_message(&itens);
        assert!(m.starts_with("continue"));
        assert!(m.contains("scaffold da skill"), "inclui o 1º item");
        assert!(m.contains("schematize skills new"), "inclui o 2º item");
        assert!(m.contains("NÃO PARE"), "reforça o não-parar");
        assert!(m.ends_with('\n'));
    }

    #[test]
    fn nudge_vazio_nao_tem_lista() {
        let m = nudge_message(&[]);
        assert!(!m.contains("Revise e feche"), "sem itens, não anexa lista");
        assert_eq!(m, "continue\n");
    }

    #[test]
    fn prompt_inicial_liga_o_overdev() {
        // Prompt em linguagem NATURAL (não o slash): sempre cita o modo OVERDEV e o
        // CHECKLIST; com objetivo, inclui o objetivo.
        let com = overdev_prompt("meu objetivo");
        assert!(com.contains("OVERDEV"));
        assert!(com.contains("CHECKLIST"));
        assert!(com.contains("meu objetivo"));
        // Objetivo vazio => sem a cláusula "O objetivo é:".
        let vazio = overdev_prompt("   ");
        assert!(vazio.contains("OVERDEV"));
        assert!(!vazio.contains("O objetivo é:"), "objetivo vazio não anexa alvo");
    }

    #[test]
    fn command_line_mostra_o_agente() {
        let cl = ClaudeRunner.command_line("obj X");
        assert!(cl.contains("claude"));
        assert_eq!(ClaudeRunner.name(), "claude");
    }
}
