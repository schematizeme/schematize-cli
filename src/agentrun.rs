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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
        s.push_str("\nNÃO PARE — ainda há itens de máquina abertos no .schematize/overdev/CHECKLIST.md. Revise e feche estes:");
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
        "Modo OVERDEV neste projeto.{alvo}\n\n\
         PASSO 0 — COMPLETUDE DO CHECKLIST, ANTES DE TICAR QUALQUER COISA. Leia \
         `.schematize/overdev/CHECKLIST.md` (e `OBJETIVO.md`, `PLAN.md`, `DECISOES.md` se existirem) e \
         responda: ele cobre o objetivo INTEIRO, ou só a primeira fase? Se estiver PARCIAL/FASEADO — \
         se você prevê que vai precisar ACRESCENTAR itens depois de fechar os atuais — DERIVE AGORA \
         os itens que faltam, de TODAS as fases, e grave no CHECKLIST antes de começar. Um item por \
         linha, cada um com como PROVAR. É proibido encerrar turno para 'planejar a próxima fase': \
         planejar é agora, executar é depois.\n\n\
         PASSO 1 — EXECUTE. Trabalhe cada `- [ ]` até fechar, marcando `- [x]` só COM PROVA \
         (teste/comando/arquivo/gate que passa). Itens `- [H ]` são de humano — não os faça. Se \
         descobrir trabalho novo no meio, ACRESCENTE ao checklist e siga; nunca pare por causa disso.\n\n\
         NÃO PARE enquanto houver `- [ ]` aberto — o Stop hook te barra de qualquer forma, e um \
         supervisor relança você se o processo morrer. Se travar numa dúvida, `schematize overdev park` \
         e siga (nunca abra pergunta bloqueante). Comece pelo PASSO 0 agora."
    )
}

/// Prompt em linguagem NATURAL pra (RE)INDEXAR o projeto (o grafo/MAPA §39). NÃO é o
/// slash `/eng-index` (que não dispara como argumento do `claude`) — dá a instrução direta
/// pra a GUI/CLI dispararem via `launch_prompt_in_terminal`. PURA.
/// Prompt (linguagem natural) que dispara a GERAÇÃO DE AFAZERES do overdev a partir do histórico
/// recuperável do archive (skill schematize-archive `/archive-todos`). Consumido pela GUI (botão).
pub fn archive_todos_prompt() -> String {
    "Gere os AFAZERES do overdev a partir do histórico RECUPERÁVEL deste projeto, seguindo a skill \
     schematize-archive (/archive-todos). Se não existir `<projeto>_archive/`, crie primeiro \
     (/archive-init — o archive é CRITICIDADE 0, obrigatório, nunca opcional). Varra EXAUSTIVAMENTE o \
     `<projeto>_archive/` + `.schematize/` + o histórico git: checklists com `- [ ]`/`- [~]`, perguntas \
     parkeadas (PERGUNTAS-OVERDEV.txt), premature-stops, ADRs `proposed`, planos com itens abertos, \
     handoffs de `chats/` com seção EM ABERTO, TODOs/FIXMEs do git. Disciplina red-first: 'feito' sem \
     prova volta a ABERTO; on-hold sem resposta = aberto. Derive um CHECKLIST exaustivo (1 item/linha, \
     cada um com COMO PROVAR, convenção de 2 níveis) agrupado por origem, grave em \
     `.schematize/overdev/CHECKLIST.md` (+ OBJETIVO.md + espelho no `<projeto>_archive/overdev/`) e \
     reporte a contagem por fonte. NÃO pare até o checklist estar completo e consistente."
        .to_string()
}

pub fn reindex_prompt() -> String {
    "Rode o índice/grafo deste projeto seguindo a §39 da engenharia da casa, gerando um GRAFO \
     GLOBAL da aplicação (não um por microserviço solto). REGRAS OBRIGATÓRIAS:\n\
     \n\
     1) GRAFO GLOBAL — SEMPRE gere `.schematize/grafos/GRAFO_GLOBAL.md`. Se esta pasta for uma \
     APLICAÇÃO multi-repo (umbrella com vários microserviços/sub-repos), o grafo global deve ter \
     CADA microserviço como nó, mostrando suas FUNÇÕES PRINCIPAIS (entrypoints/APIs públicas), e as \
     arestas de CONTRATO entre serviços (a saída de dados do serviço A para o B). Enumere TODOS os \
     sub-repos — nenhum de fora. Se for um único serviço, o global traz esse serviço e as arestas \
     que cruzam a fronteira dele.\n\
     2) GRAFO POR MICROSERVIÇO — gere um arquivo detalhado por serviço em \
     `.schematize/grafos/<servico>.md`: funções internas como nós, chamadas intra-serviço como \
     arestas, cada nó com `arquivo:linha`.\n\
     3) AUTO-REFERÊNCIA DE FRONTEIRA — quando uma função de um serviço produz saída de dados para \
     OUTRO serviço, marque esse nó no grafo local como saída (`-> <outro-servico>`) apontando pro \
     grafo global (a aresta que sai do grafo local).\n\
     4) FORMATO (casar com o parser do app): arestas SEMPRE em ASCII `A -> B (contrato)` — NUNCA a \
     seta unicode `→`. Nós de função em tabela pipe `nome | o quê | ... | arquivo:linha`. Cada nó \
     tem uma descrição de uma linha (a coluna \"O quê\").\n\
     5) ESPELHO no archive: copie `GRAFO_GLOBAL.md` + `INDEX_GLOBAL.md` + `INDEX_FUNCTIONS.md` \
     também para `<projeto>_archive/index/` (registro durável). A versão OPERACIONAL viva é a de \
     `.schematize/grafos/`.\n\
     \n\
     Confira contra o código: nenhum nó órfão, nenhuma função pública sem entrada. NÃO pare até o \
     grafo global e os por-serviço estarem completos e consistentes."
        .to_string()
}

/// `true` se o CLL `claude` está no `$PATH` (checagem barata pra a GUI/CLI
/// decidirem entre disparar a sessão ou só imprimir a dica).
pub fn claude_in_path() -> bool {
    binary_in_path("claude")
}

/// Caminho ABSOLUTO do `claude` resolvido pelo `$PATH` + dirs de fallback (o mesmo que o app usa
/// pra spawnar). `None` se não achar. Exposto pro debug report não divergir do que o launch enxerga.
pub fn claude_path() -> Option<std::path::PathBuf> {
    resolve_bin("claude")
}

/// Diretórios onde ferramentas de usuário caem mas que o PATH de um processo
/// aberto pelo LANÇADOR DO DESKTOP não inclui — o desktop não carrega
/// `~/.profile`/`~/.bashrc`. Checar aqui (além do `$PATH`) é o que faz o app
/// achar o `claude` mesmo quando foi aberto pelo menu de apps e não pelo terminal.
fn fallback_bin_dirs() -> Vec<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut dirs = Vec::new();
    if let Some(h) = home {
        dirs.push(h.join(".local/bin"));
        dirs.push(h.join(".claude/local"));
        dirs.push(h.join(".cargo/bin"));
    }
    dirs.push(std::path::PathBuf::from("/usr/local/bin"));
    dirs.push(std::path::PathBuf::from("/usr/bin"));
    dirs.push(std::path::PathBuf::from("/opt/homebrew/bin"));
    dirs
}

/// Resolve o caminho ABSOLUTO de `bin`: primeiro nos diretórios do `$PATH` do
/// processo, depois nos de fallback ([`fallback_bin_dirs`]). `None` se não achar
/// em lugar nenhum. É o que permite achar/rodar o `claude` com o PATH mínimo da GUI.
fn resolve_bin(bin: &str) -> Option<std::path::PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(bin);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    fallback_bin_dirs()
        .into_iter()
        .map(|dir| dir.join(bin))
        .find(|p| p.is_file())
}

/// `true` se `bin` existe no `$PATH` OU nos diretórios de fallback — checagem
/// barata pra dar erro claro antes de tentar spawnar um agente inexistente.
fn binary_in_path(bin: &str) -> bool {
    resolve_bin(bin).is_some()
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
    let term = launch_prompt_in_terminal(project, &overdev_prompt(objetivo))?;
    // Rede de segurança: o Stop hook só age quando o agente TENTA encerrar o turno; ele não
    // pode nada se o processo simplesmente morre (contexto, compactação, crash, janela
    // fechada). O supervisor cobre isso relançando enquanto houver `- [ ]`. Sobe destacado e
    // é idempotente. NÃO fica no `launch_prompt_in_terminal` de propósito: aquele é genérico
    // (reindex, partes do split) e o próprio supervisor o usa pra relançar — supervisionar
    // ali seria recursão.
    crate::overdev::supervisor::garantir_supervisor(project);
    Ok(term)
}

/// Sequência monotônica de lançamentos DENTRO deste processo. Junto do pid e do relógio,
/// dá o par de arquivos exclusivo de cada `launch_prompt_in_terminal` (ver
/// [`arquivos_de_lancamento`]).
static LANCAMENTO_SEQ: AtomicU64 = AtomicU64::new(0);

/// Par de arquivos EXCLUSIVO de um lançamento: `(prompt, wrapper)`.
///
/// ## Por que existe (o bug que ela mata)
/// Os dois arquivos eram de nome FIXO (`run-prompt.txt`, `run.sh`) e o wrapper lê o prompt
/// com `$(cat …)` **quando o terminal sobe**, não quando o app grava. Como `overdev split
/// --dispatch` chama isto K vezes em sequência e `spawn` não espera o terminal subir, o app
/// (microssegundos por escrita) sempre vencia o emulador de terminal (centenas de
/// milissegundos até o `cat`): **todos** os K agentes liam a ÚLTIMA fatia escrita. Com K=2,
/// dois agentes no `part-02` e o `part-01` órfão — com os itens dele já movidos pra fora do
/// `CHECKLIST.md` primário, ou seja, ninguém trabalhando neles. Não era intermitente: o app
/// ganhava a corrida em toda execução.
///
/// ## Comportamento
/// Nome único por `pid`+relógio+sequência: `pid` separa processos, o relógio cobre reuso de
/// pid depois que o processo morre, e a sequência separa os lançamentos do MESMO processo —
/// que é exatamente o caso do split. Pura de propósito (só monta caminho, não toca disco),
/// pra ser testável sem abrir terminal.
///
/// **Entrada:** `od` — o dir do control-plane (`.schematize/overdev/`).
/// **Saída:** `(promptfile, script)`, ambos dentro de `od`, distintos a cada chamada.
/// **Efeitos:** nenhum no disco; só incrementa o contador em memória.
fn arquivos_de_lancamento(od: &Path) -> (PathBuf, PathBuf) {
    let seq = LANCAMENTO_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let token = format!("{pid}-{nanos:x}-{seq}");
    (od.join(format!("run-prompt-{token}.txt")), od.join(format!("run-{token}.sh")))
}

/// Apaga os pares de lançamento com mais de 24h do control-plane.
///
/// Por que: o nome único (§ [`arquivos_de_lancamento`]) resolve a corrida mas acumula
/// arquivo — o wrapper precisa sobreviver até o terminal lê-lo, então não dá pra apagar na
/// saída. Limpar o que é velho o bastante pra nenhum terminal ainda estar subindo mantém o
/// dir enxuto sem janela de corrida. Leva junto os nomes FIXOS legados (`run.sh`,
/// `run-prompt.txt`), resíduo das versões anteriores.
///
/// **Entrada:** `od` — o dir do control-plane. **Saída:** nenhuma (best-effort).
/// **Efeitos:** remove arquivo do disco; erro é ignorado de propósito — falhar a limpeza
/// nunca pode impedir um lançamento.
fn purga_lancamentos_velhos(od: &Path) {
    const VELHO_SEGS: u64 = 24 * 60 * 60;
    let agora = std::time::SystemTime::now();
    for e in std::fs::read_dir(od).into_iter().flatten().flatten() {
        let p = e.path();
        let Some(n) = p.file_name().and_then(|n| n.to_str()) else { continue };
        let alvo = (n.starts_with("run-prompt-") && n.ends_with(".txt"))
            || (n.starts_with("run-") && n.ends_with(".sh"))
            || n == "run.sh"
            || n == "run-prompt.txt";
        if !alvo {
            continue;
        }
        let velho = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| agora.duration_since(t).ok())
            .is_some_and(|d| d.as_secs() > VELHO_SEGS);
        if velho {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// Igual à [`launch_in_terminal`], mas dispara um PROMPT ARBITRÁRIO (linguagem natural) num
/// terminal externo — reusada pelo (re)index ([`reindex_prompt`]) e por qualquer ação one-shot
/// que a GUI/CLI queira delegar ao `claude` fora do processo do app. Escreve o prompt num arquivo
/// (evita quoting de shell) + um wrapper `.overdev/run.sh` e abre o 1º terminal disponível.
/// Devolve o nome do terminal usado. Erro se `claude` ou nenhum terminal estiver no PATH.
pub fn launch_prompt_in_terminal(project: &Path, prompt: &str) -> Result<String, String> {
    let claude = resolve_bin("claude").ok_or_else(|| {
        "o CLI `claude` não está no PATH — instale o Claude Code (claude.ai/code).".to_string()
    })?;
    pre_trust_project(project);
    // Control-plane operacional: `.schematize/overdev/` (cai no `.overdev/` legado se o projeto
    // ainda estiver no layout antigo — resolvido pelo módulo `paths`).
    let od = crate::paths::overdev_dir_at(project);
    std::fs::create_dir_all(&od).map_err(|e| format!("criar .schematize/overdev: {e}"))?;
    purga_lancamentos_velhos(&od);
    let (promptfile, script) = arquivos_de_lancamento(&od);
    std::fs::write(&promptfile, prompt).map_err(|e| format!("gravar prompt: {e}"))?;
    // O terminal roda `bash run.sh` (non-login): NÃO lê ~/.bashrc, então reforçamos o PATH com os
    // dirs de usuário (o próprio claude, node e ripgrep resolvem daqui) e chamamos o claude pelo
    // caminho ABSOLUTO — assim funciona mesmo com o app aberto pelo lançador do desktop.
    let sh = format!(
        "#!/usr/bin/env bash\nexport PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.claude/local:$PATH\"\ncd {proj:?} || exit 1\n{claude:?} --dangerously-skip-permissions \"$(cat {pf:?})\"\necho\nread -rp '[overdev encerrado — Enter para fechar] '\n",
        proj = project,
        claude = claude,
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
        let claude = resolve_bin("claude").ok_or_else(|| {
            "o CLI `claude` não está no PATH. Instale o Claude Code (claude.ai/code) \
             e garanta que `claude` roda no terminal antes de usar `overdev run`."
                .to_string()
        })?;
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("falha ao abrir PTY: {e}"))?;

        // Caminho ABSOLUTO do claude (o portable-pty resolve o binário na hora do spawn; com o PATH
        // mínimo da GUI, "claude" nu não resolveria). O PATH do filho é reforçado logo abaixo.
        let mut cmd = CommandBuilder::new(claude);
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
    println!("[schematize] monitorando o progresso em {}/.schematize/overdev — ocioso {IDLE_THRESHOLD_SECS}s com item aberto => injeta `continue` (até {max}x).\n", project.display());

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

    #[test]
    fn resolve_bin_acha_no_path_e_da_absoluto() {
        // `sh` existe em toda máquina POSIX e está no $PATH: resolve pra caminho absoluto de arquivo.
        let p = resolve_bin("sh").expect("sh resolve");
        assert!(p.is_absolute() && p.is_file(), "resolve_bin deve dar arquivo absoluto: {p:?}");
        // Binário inexistente não resolve (nem no PATH nem no fallback).
        assert!(resolve_bin("binario-que-nao-existe-xyz").is_none());
    }

    #[test]
    fn resolve_bin_acha_no_fallback_fora_do_path() {
        // Simula o PATH mínimo da GUI (sem ~/.local/bin) e um bin plantado num dir de fallback:
        // resolve_bin ainda acha porque varre os fallback_bin_dirs (via HOME).
        use std::io::Write;
        let tmp = std::env::temp_dir().join(format!("schz-resolve-{}", std::process::id()));
        let localbin = tmp.join(".local/bin");
        std::fs::create_dir_all(&localbin).unwrap();
        let fake = localbin.join("claude-fake-xyz");
        let mut f = std::fs::File::create(&fake).unwrap();
        f.write_all(b"#!/bin/sh\n").unwrap();
        drop(f);
        let prev_home = std::env::var_os("HOME");
        let prev_path = std::env::var_os("PATH");
        // SAFETY: teste single-thread neste módulo; restauramos logo abaixo.
        unsafe {
            std::env::set_var("HOME", &tmp);
            std::env::set_var("PATH", "/usr/bin:/bin"); // PATH mínimo, sem o fallback
        }
        let got = resolve_bin("claude-fake-xyz");
        unsafe {
            match prev_home {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
            match prev_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(got.as_deref(), Some(fake.as_path()), "deve achar no fallback ~/.local/bin");
    }

    /// O QUE: prova que dois lançamentos seguidos NÃO compartilham arquivo.
    ///
    /// POR QUE: com nome fixo (`run-prompt.txt`/`run.sh`), o 2º lançamento sobrescrevia o 1º
    /// antes de o terminal do 1º ler — e os K agentes do `overdev split --dispatch` recebiam
    /// todos a última fatia. Esta asserção reprova se alguém voltar ao nome fixo.
    #[test]
    fn cada_lancamento_ganha_seu_par_de_arquivos() {
        let od = Path::new("/tmp/sz-lanc-teste");
        let (p1, s1) = arquivos_de_lancamento(od);
        let (p2, s2) = arquivos_de_lancamento(od);

        assert_ne!(p1, p2, "dois lançamentos gravaram no MESMO arquivo de prompt");
        assert_ne!(s1, s2, "dois lançamentos gravaram no MESMO wrapper");
        assert_ne!(p1, s1, "prompt e wrapper não podem ser o mesmo arquivo");
        for f in [&p1, &s1, &p2, &s2] {
            let n = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            assert!(n != "run.sh" && n != "run-prompt.txt", "voltou ao nome FIXO: {n}");
            assert_eq!(f.parent(), Some(od), "arquivo saiu do control-plane");
        }
    }

    /// O QUE: reproduz o `overdev split --dispatch` com K fatias e prova que cada terminal lê
    /// A SUA — a corrida real, não só a unicidade do nome.
    ///
    /// COMO: escreve os K prompts como o dispatch faz (em sequência, sem esperar ninguém) e só
    /// DEPOIS lê todos, que é a ordem que o bug expunha — o terminal só faz `cat` centenas de
    /// milissegundos após o app ter escrito o próximo. Com nome fixo, os K lidos seriam todos
    /// iguais ao último; aqui têm que ser exatamente os K escritos.
    #[test]
    fn dispatch_de_k_fatias_nao_embaralha_os_prompts() {
        const K: usize = 5;
        let od = std::env::temp_dir().join(format!("sz-dispatch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&od);

        // Fase 1 — o app escreve as K fatias em rajada (nenhum "terminal" leu ainda).
        let mut escritos = Vec::new();
        for i in 1..=K {
            let (pf, _sh) = arquivos_de_lancamento(&od);
            let corpo = format!("cuide APENAS de checklist/part-{i:02}.md");
            std::fs::write(&pf, &corpo).expect("gravar prompt");
            escritos.push((pf, corpo));
        }

        // Fase 2 — só agora os terminais sobem e fazem o `cat`.
        for (pf, esperado) in &escritos {
            let lido = std::fs::read_to_string(pf).expect("ler prompt");
            assert_eq!(&lido, esperado, "terminal leu a fatia de outro agente");
        }
        let distintos: std::collections::HashSet<_> =
            escritos.iter().map(|(_, c)| c.as_str()).collect();
        assert_eq!(distintos.len(), K, "as {K} fatias não chegaram distintas aos agentes");

        let _ = std::fs::remove_dir_all(&od);
    }
}
