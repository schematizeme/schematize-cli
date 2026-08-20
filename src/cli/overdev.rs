//! Subcomandos do OVERDEV: rodar, dividir em K agentes, snapshot/histórico/restore
//! do DB local e o log de conclusões.

use schematize::{
    overdev, overdevdb,
};
use schematize::agentrun;
use schematize::agentrun::AgentRunner;
use crate::cli::ssh::confirm;

/// `schematize overdev run [--max N] [--yes]` — dispara o `claude` acoplado no
/// diretório atual e monitora (auto-continue). Guardrail: mostra o comando do
/// agente e confirma antes (o agente MEXE no projeto), a menos de `--yes`.
/// `schematize overdev split K` — divide o checklist em K parts e (com --dispatch) lança K claudes,
/// tudo dentro do teto seguro do governador (`schematize agents`).
pub(crate) fn overdev_split(k: usize, dispatch: bool, force: bool) -> Result<(), String> {
    let project = std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))?;
    let b = schematize::agents::budget();
    let plan = b.split_plan(k);

    println!("Governador de concorrência (máquina inteira):");
    println!("  teto seguro: {} · rodando agora: {} · disponível: {}", b.total_cap, b.snap.running_claudes, b.available);
    println!("  split em {} claude(s): {} subagents cada (total {}/{} do teto)", plan.mains, plan.subagents_each, plan.total_used, plan.cap);

    if k > b.total_cap && !force {
        return Err(format!(
            "{k} claudes principais passa do teto seguro ({}). Reduza o K ou use --force (pode travar a máquina).",
            b.total_cap
        ));
    }
    if dispatch && b.available < k && !force {
        return Err(format!(
            "só há {} slot(s) livre(s) na máquina agora (teto {} − {} rodando); lançar {k} travaria. Espere liberar ou use --force.",
            b.available, b.total_cap, b.snap.running_claudes
        ));
    }

    let res = overdev::split(&project, k)?;
    println!("\n✓ dividido: {} itens em {} parte(s):", res.moved, res.parts.len());
    for (i, (f, n)) in res.parts.iter().zip(&res.per_part).enumerate() {
        println!("  part {:>2}: {n:>3} item(ns)  → {}", i + 1, f.display());
    }

    if !dispatch {
        println!("\nRevise os parts e rode com --dispatch pra lançar os {k} claudes (ou abra cada um você mesmo).");
        return Ok(());
    }

    println!("\nLançando {k} claude(s) — um por part…");
    for (i, f) in res.parts.iter().enumerate() {
        let rel = f.strip_prefix(&project).unwrap_or(f);
        let prompt = format!(
            "Rode o overdev deste projeto cuidando APENAS do arquivo `{}` (sua fatia do split). Feche \
             TODOS os itens `- [ ]` dele com prova, seguindo a disciplina do overdev. Você pode usar até \
             {} subagents em paralelo — NÃO ultrapasse, pra não travar a máquina (há outros claudes \
             rodando as outras fatias). Não toque nos outros part-*.md.",
            rel.display(),
            plan.subagents_each
        );
        match agentrun::launch_prompt_in_terminal(&project, &prompt) {
            Ok(_) => println!("  ✓ claude {} lançado ({})", i + 1, rel.display()),
            Err(e) => println!("  ✗ claude {} falhou: {e}", i + 1),
        }
    }
    Ok(())
}

pub(crate) fn overdev_run(max: Option<u64>, yes: bool) -> Result<(), String> {
    let project = std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))?;
    let max = max.unwrap_or(agentrun::DEFAULT_MAX_NUDGES);
    let objetivo = overdev::objetivo_at(&project).unwrap_or_default();
    let runner = agentrun::ClaudeRunner;
    println!("Vai disparar o agente acoplado neste projeto:");
    println!("  projeto: {}", project.display());
    println!("  comando: {}", runner.command_line(&objetivo));
    println!("  auto-continue: até {max} nudge(s) quando o agente ficar ocioso com item aberto.");
    if !yes && !confirm("Disparar o agente `claude` acoplado neste projeto? [s/N]") {
        return Err("cancelado.".to_string());
    }
    agentrun::run_attached(&project, &runner, max)
}

/// cwd como raiz do projeto (erro claro se inacessível).
pub(crate) fn cwd_project() -> Result<std::path::PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))
}

/// `schematize overdev snapshot` — grava no DB local as versões novas dos artefatos.
pub(crate) fn overdev_snapshot() -> Result<(), String> {
    let project = cwd_project()?;
    let n = overdevdb::snapshot(&project)?;
    if n == 0 {
        println!("nenhuma mudança — nada novo pra versionar.");
    } else {
        println!("{n} snapshot(s) novo(s) gravado(s) no DB local.");
    }
    Ok(())
}

/// `schematize overdev history [--limit N]` — tabela do histórico do projeto.
pub(crate) fn overdev_history(limit: usize) -> Result<(), String> {
    let project = cwd_project()?;
    let hist = overdevdb::history(&project, limit)?;
    if hist.is_empty() {
        println!("sem snapshots pra este projeto ainda (rode `schematize overdev snapshot`).");
        return Ok(());
    }
    println!("{:>6}  {:<19}  {:>8}  {}", "id", "quando", "bytes", "arquivo");
    for m in hist {
        println!("{:>6}  {:<19}  {:>8}  {}", m.id, fmt_ts(m.ts), m.size, m.file);
    }
    println!("(veja um: `schematize overdev restore <id>`)");
    Ok(())
}

/// `schematize overdev restore <id>` — regrava o snapshot no caminho original.
pub(crate) fn overdev_restore(id: i64) -> Result<(), String> {
    let project = cwd_project()?;
    let dest = overdevdb::restore(id, &project)?;
    println!("snapshot {id} restaurado em {}", dest.display());
    Ok(())
}

/// Formata um epoch secs em `AAAA-MM-DD HH:MM` (UTC, sem crate de data).
pub(crate) fn fmt_ts(ts: i64) -> String {
    // Cálculo civil a partir do epoch (algoritmo de Howard Hinnant), UTC.
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let (h, mi) = (secs / 3600, (secs % 3600) / 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

/// Offset local em segundos (via `date +%z`, ex.: `-0300`). Fallback 0 (UTC) se
/// o `date` falhar ou vier num formato inesperado.
pub(crate) fn local_offset_secs() -> i64 {
    let out = match std::process::Command::new("date").arg("+%z").output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return 0,
    };
    let s = String::from_utf8_lossy(&out);
    let s = s.trim();
    // Formato esperado: sinal + HHMM (ex.: "-0300", "+0530").
    if s.len() < 5 {
        return 0;
    }
    let sign: i64 = if s.starts_with('-') { -1 } else { 1 };
    let digits: Vec<char> = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return 0;
    }
    let hh: i64 = format!("{}{}", digits[0], digits[1]).parse().unwrap_or(0);
    let mm: i64 = format!("{}{}", digits[2], digits[3]).parse().unwrap_or(0);
    sign * (hh * 3600 + mm * 60)
}

/// Hora local `HH:MM:SS` de um epoch (secs), dado o offset local. PURO/testável.
pub(crate) fn fmt_hms(ts: i64, offset: i64) -> String {
    let secs = (ts + offset).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// `schematize overdev log` — lista as conclusões (`- [x]`) do CHECKLIST com a hora
/// local em que foram detectadas (lê `.overdev/completions.json`, sem gravar).
pub(crate) fn overdev_log() {
    let project = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let comps = overdev::completions(&project);
    if comps.is_empty() {
        println!("sem conclusões registradas ainda (.schematize/overdev/completions.json).");
        return;
    }
    let off = local_offset_secs();
    println!("conclusões ({}):", comps.len());
    for c in &comps {
        println!("  {}  {}", fmt_hms(c.ts, off), c.text);
    }
}

/// `schematize overdev load|index` — dispara uma sessão `claude` one-shot no cwd
/// com o comando dado. Se `claude` não estiver no PATH, só imprime a dica.
pub(crate) fn overdev_agent_cmd(cmd: &str) -> Result<(), String> {
    if !agentrun::claude_in_path() {
        println!("`claude` não está no PATH. Rode manualmente na pasta do projeto: claude {cmd}");
        return Ok(());
    }
    println!("disparando sessão `claude` no diretório atual com: {cmd}");
    match std::process::Command::new("claude").arg(cmd).status() {
        Ok(st) if st.success() => Ok(()),
        Ok(st) => {
            println!("a sessão `claude` saiu com {st}. Se preciso, rode à mão: claude {cmd}");
            Ok(())
        }
        Err(e) => {
            println!("não consegui disparar `claude` ({e}). Rode à mão: claude {cmd}");
            Ok(())
        }
    }
}
