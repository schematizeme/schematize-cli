//! LANÇADOR: achar o binário do `claude`, pré-confiar a pasta e abrir terminal externo.
//!
//! O quê: resolução de binário fora do PATH (o lançador do desktop dá PATH mínimo), o
//! pré-consentimento da pasta no config do Claude Code, os arquivos ÚNICOS por lançamento e
//! as formas de abrir terminal — tarefa com prompt, overdev com supervisor, e sessão
//! interativa pro humano.
//! Onde: CLI (`overdev run`/`terminal`/`split`) e GUI (botões da aba Overdev).
//!
//! Separado porque é tudo dependente de PLATAFORMA (terminais, permissão, grupo de
//! processos) — muda por razão de sistema operacional, não de produto.

use super::*;

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
pub(crate) fn fallback_bin_dirs() -> Vec<std::path::PathBuf> {
    // `util::home()` e não `env HOME`: no Windows a variável é outra, e a resolução certa
    // mora num lugar só (ver o doc de `util::home`).
    let h = crate::util::home();
    let mut dirs = vec![h.join(".local/bin"), h.join(".claude/local"), h.join(".cargo/bin")];
    #[cfg(windows)]
    {
        // O OpenSSH do Windows vem aqui desde o Windows 10 1803, e NÃO está no PATH de todo
        // perfil — sem este fallback, o `vps exec` falha em máquina recém-instalada.
        if let Some(sysroot) = std::env::var_os("SystemRoot") {
            let sr = std::path::PathBuf::from(sysroot);
            dirs.push(sr.join("System32").join("OpenSSH"));
            dirs.push(sr.join("SysNative").join("OpenSSH"));
        }
        for pf in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(d) = std::env::var_os(pf) {
                dirs.push(std::path::PathBuf::from(d).join("OpenSSH"));
            }
        }
    }
    dirs.push(std::path::PathBuf::from("/usr/local/bin"));
    dirs.push(std::path::PathBuf::from("/usr/bin"));
    dirs.push(std::path::PathBuf::from("/opt/homebrew/bin"));
    dirs.push(std::path::PathBuf::from("/usr/local/opt/openssh/bin"));
    dirs
}

/// Resolve o caminho ABSOLUTO de `bin`: primeiro nos diretórios do `$PATH` do
/// processo, depois nos de fallback ([`fallback_bin_dirs`]). `None` se não achar
/// em lugar nenhum. É o que permite achar/rodar o `claude` com o PATH mínimo da GUI.
pub(crate) fn resolve_bin(bin: &str) -> Option<std::path::PathBuf> {
    for dir in std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(fallback_bin_dirs())
    {
        for nome in nomes_de_executavel(bin) {
            let p = dir.join(&nome);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Nomes de arquivo que um executável `bin` pode ter neste sistema.
///
/// No Unix é só o nome. **No Windows o arquivo é `ssh.exe`**, e a versão anterior procurava
/// `ssh` — nunca achava. Como todo `vps exec` passa por `resolve_bin("ssh")`, o gestor de VPS
/// simplesmente não funcionava no Windows, que é uma das plataformas que o app publica
/// (`selfupdate.rs` distribui `schematize-windows-x86_64.exe`).
///
/// A lista segue o `PATHEXT` padrão, na ordem em que o próprio Windows resolve.
fn nomes_de_executavel(bin: &str) -> Vec<String> {
    nomes_de_executavel_em(bin, cfg!(windows))
}

/// A REGRA de [`nomes_de_executavel`], com a plataforma como PARÂMETRO.
///
/// Existe separada porque a versão anterior era `#[cfg(windows)]`: no Linux o corpo nem
/// compilava, então **nenhum teste podia exercitá-lo** — o mutation testing flagrou que
/// desligar a correção do Windows não quebrava teste nenhum. Com a plataforma como argumento,
/// a regra é verificável em qualquer máquina, e o `cfg!` só escolhe o argumento.
///
/// **Onde:** [`nomes_de_executavel`] em produção, e os testes com os dois valores.
pub fn nomes_de_executavel_em(bin: &str, windows: bool) -> Vec<String> {
    if !windows {
        return vec![bin.to_string()];
    }
    // Já veio com extensão: respeita o que o chamador pediu.
    if bin.contains('.') {
        return vec![bin.to_string()];
    }
    let mut v = vec![bin.to_string()];
    for ext in [".exe", ".cmd", ".bat", ".com"] {
        v.push(format!("{bin}{ext}"));
    }
    v
}

/// `true` se `bin` existe no `$PATH` OU nos diretórios de fallback — checagem
/// barata pra dar erro claro antes de tentar spawnar um agente inexistente.
pub(crate) fn binary_in_path(bin: &str) -> bool {
    resolve_bin(bin).is_some()
}

/// Marca `projects.<caminho>.hasTrustDialogAccepted = true` no `~/.claude.json` pra o
/// `claude` NÃO exibir o "Is this a project you trust?" (que trava o run acoplado). Best-effort:
/// qualquer erro é ignorado (no pior caso o agente só mostra o prompt, como antes). Preserva todo
/// o resto do config (merge por campo). Não é campo de segurança do nosso lado — o usuário
/// disparou o overdev no próprio projeto.
pub(crate) fn pre_trust_project(project: &Path) {
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
    let projects = obj.entry("projects").or_insert_with(|| serde_json::json!({}));
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
pub(crate) fn arquivos_de_lancamento(od: &Path) -> (PathBuf, PathBuf) {
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
pub(crate) fn purga_lancamentos_velhos(od: &Path) {
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

/// Terminais aceitos, em ordem de preferência. Um só lugar: a lista estava duplicada aqui e
/// no `selfupdate`, e listas duplicadas divergem.
const TERMINAIS: [&str; 7] = [
    "konsole",
    "gnome-terminal",
    "xfce4-terminal",
    "x-terminal-emulator",
    "alacritty",
    "kitty",
    "xterm",
];

/// Abre `script` no primeiro terminal disponível, desacoplado deste processo.
///
/// O quê: acha o terminal, monta os args (o `--` de gnome/xfce vs o `-e` dos outros) e
/// spawna em grupo de processos próprio. Onde: [`launch_prompt_in_terminal`] e
/// [`abrir_terminal_no_projeto`].
/// **Saída:** o nome do terminal usado, ou erro se não houver nenhum.
/// **Efeitos:** cria processo.
/// Abre um COMANDO arbitrário num terminal do sistema, desacoplado deste processo.
///
/// **Onde:** `vps::conexao::abrir_no_terminal` (o botão "Abrir no terminal" da GUI). Envolve
/// o comando num script temporário porque é isso que [`spawn_no_terminal`] sabe lançar em
/// todos os emuladores suportados — cada um tem sua própria forma de receber um comando, e o
/// script normaliza a diferença.
pub fn abrir_comando_no_terminal(comando: &str) -> Result<String, String> {
    let dir = crate::util::home_app_dir().join("run");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("não consegui criar {}: {e}", dir.display()))?;
    let script = dir.join(format!("term-{}.sh", std::process::id()));
    // `exec` no fim: o shell do wrapper some e o terminal fica com o processo de verdade,
    // então fechar a janela encerra a sessão em vez de deixar um ssh órfão.
    std::fs::write(&script, format!("#!/bin/sh\nexec {comando}\n"))
        .map_err(|e| format!("não consegui gravar {}: {e}", script.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700));
    }
    spawn_no_terminal(&script)
}

pub(crate) fn spawn_no_terminal(script: &Path) -> Result<String, String> {
    let term = TERMINAIS.iter().find(|t| binary_in_path(t)).ok_or_else(|| {
        "nenhum terminal encontrado (konsole/gnome-terminal/xterm/…).".to_string()
    })?;
    let mut cmd = std::process::Command::new(term);
    match *term {
        // esses usam `--` pra separar o comando a executar
        "gnome-terminal" | "xfce4-terminal" => {
            cmd.arg("--").arg("bash").arg(script);
        }
        _ => {
            cmd.arg("-e").arg("bash").arg(script);
        }
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // desacopla do app: sobrevive se o app fechar
    }
    cmd.spawn().map_err(|e| format!("abrir terminal `{term}`: {e}"))?;
    Ok((*term).to_string())
}

/// Abre um terminal INTERATIVO já dentro do projeto, com o `claude` pronto e o bypass de
/// permissões ligado — o "vou trabalhar neste projeto agora".
///
/// O quê: grava um wrapper (nome único, ver [`arquivos_de_lancamento`]), pré-confia a pasta
/// e abre o terminal. O wrapper corrige o PATH (terminal non-login não lê `~/.bashrc`),
/// entra no projeto, roda o `claude --dangerously-skip-permissions` INTERATIVO (sem prompt
/// pré-fabricado) e, quando ele sai, **cai num shell no mesmo diretório** em vez de fechar
/// a janela.
///
/// Onde: botão "abrir no terminal" da GUI; `schematize overdev terminal`.
///
/// ## Diferença pro [`launch_prompt_in_terminal`]
/// Aquele dispara UMA tarefa (o agente recebe um prompt e trabalha sozinho). Este entrega o
/// terminal pro humano: sessão interativa, sem objetivo embutido, e o shell continua vivo
/// depois. São intenções diferentes e por isso são funções diferentes — e este NÃO sobe
/// supervisor, porque não há run de overdev pra vigiar.
///
/// ## Sem `claude` instalado
/// Não é erro: abre o shell no projeto do mesmo jeito e avisa na tela como instalar. Quem
/// clicou quer chegar na pasta certa; falhar por causa de uma dependência opcional seria
/// culpar o usuário por algo que o software resolve (§48).
///
/// **Entrada:** raiz do projeto. **Saída:** nome do terminal usado.
/// **Efeitos:** grava wrapper em `.schematize/overdev/`, cria processo.
pub fn abrir_terminal_no_projeto(project: &Path) -> Result<String, String> {
    pre_trust_project(project);
    let od = crate::paths::overdev_dir_at(project);
    std::fs::create_dir_all(&od).map_err(|e| format!("criar .schematize/overdev: {e}"))?;
    purga_lancamentos_velhos(&od);
    let (_prompt, script) = arquivos_de_lancamento(&od);

    // Absoluto quando existe; senão o wrapper avisa em vez de quebrar.
    let linha_claude = match resolve_bin("claude") {
        Some(c) => format!("if [ -x {c:?} ]; then\n  {c:?} --dangerously-skip-permissions\nfi\n"),
        None => "echo '[schematize] o CLI `claude` nao esta no PATH — instale em claude.ai/code.'\necho '[schematize] abrindo so o shell neste projeto.'\n".to_string(),
    };
    // String de UMA linha com `\n` explicito: a continuacao com `\` do Rust preserva a
    // indentacao do fonte e vazava espaco pra dentro do script gerado.
    let sh = format!(
        "#!/usr/bin/env bash\nexport PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.claude/local:$PATH\"\ncd {proj:?} || exit 1\n{linha_claude}echo\necho '[schematize] voce esta em {mostra} — o shell segue aberto.'\nexec \"${{SHELL:-/bin/bash}}\" -i\n",
        proj = project,
        mostra = project.display(),
    );
    std::fs::write(&script, &sh).map_err(|e| format!("gravar wrapper: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
    }
    spawn_no_terminal(&script)
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
    spawn_no_terminal(&script)
}

// ---------------------------------------------------------------------------
// Trait plugável + implementação default (Claude Code CLI em PTY).
// ---------------------------------------------------------------------------
