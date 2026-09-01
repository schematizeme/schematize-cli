//! SUPERVISOR do overdev — a camada que faz o run NÃO PARAR quando o Stop hook não alcança.
//!
//! O quê: um laço que vigia um run e, se o agente morreu com item de máquina ainda aberto,
//! **relança** o `claude` naquele projeto. Onde: `schematize overdev supervise`, disparado
//! automaticamente por [`crate::agentrun::launch_in_terminal`] e chamável à mão.
//!
//! ## Por que existe (o buraco que o hook não tapa)
//! O Stop hook só é consultado quando o agente **tenta encerrar o turno**. Ele não é
//! chamado — e não pode fazer nada — quando o processo simplesmente **acaba**: contexto
//! estourado, compactação que derruba, erro de API, crash, ou a pessoa fechando a janela.
//! No modo acoplado (`overdev run`) existe o auto-continue por PTY; no modo TERMINAL
//! EXTERNO, que é o que a GUI usa, não havia nada. O run morria e ficava morto até alguém
//! olhar. É exatamente o "faseado" de quem precisa voltar e reiniciar na mão.
//!
//! ## O que ele NÃO faz
//! Não burla piso: não fecha item, não marca `- [x]`, não mexe no checklist. Ele só
//! garante que exista um agente vivo enquanto houver trabalho de máquina aberto. Item
//! humano (`- [H ]`) e on-hold (`- [~]`) **não** disparam relançamento — quem fecha
//! aquilo é gente, e insistir viraria laço infinito.

use crate::agentrun;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Intervalo entre rondas. Alto de propósito: o supervisor é rede de segurança, não
/// monitor — sondar de segundo em segundo só gastaria CPU.
const RONDA_SEGS: u64 = 30;

/// Quantas rondas seguidas sem agente vivo antes de relançar. Evita relançar durante o
/// intervalo em que o terminal está subindo (o `claude` demora a aparecer no /proc).
const RONDAS_ATE_RELANCAR: u32 = 2;

/// Teto de relançamentos de uma supervisão. Guardrail anti-loop: se o agente morre toda
/// vez, o problema não é ausência de agente — é outra coisa, e insistir esconde isso.
pub const MAX_RELANCAMENTOS: u32 = 20;

/// Existe um `claude` vivo com o cwd DENTRO deste projeto?
///
/// O quê: varre `/proc/<pid>/cwd` e casa contra a raiz do projeto. Onde: [`supervise`].
/// Por que por cwd: a contagem de `crate::agents` é da máquina inteira; aqui precisamos
/// saber se **este** run tem dono, senão um claude de outro projeto mascararia a morte.
/// **Entrada:** raiz do projeto (idealmente absoluta). **Saída:** `true` se achou.
/// **Efeitos:** só leitura de `/proc`; erro de permissão é tratado como "não é este".
pub fn agente_vivo_em(projeto: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let args: Vec<String> = raw
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        if args.is_empty() || !crate::agents::cmdline_e_claude(&args) {
            continue;
        }
        if let Ok(cwd) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
            if cwd.starts_with(projeto) {
                return true;
            }
        }
    }
    false
}

/// O run ainda tem trabalho de MÁQUINA aberto?
///
/// O quê: lê o progresso do projeto e devolve `true` se `mode == "active"` e há `- [ ]`.
/// Onde: [`supervise`], como condição de continuar vigiando e de relançar.
/// **Efeitos:** lê o control-plane do projeto.
pub fn tem_trabalho_de_maquina(projeto: &Path) -> bool {
    let p = crate::overdev::progress_at(projeto);
    p.mode == "active" && p.open > 0
}

/// Prompt de RETOMADA — o que o agente relançado recebe.
///
/// O quê: manda reler o control-plane e continuar do primeiro item aberto, deixando
/// explícito que houve uma queda (pra ele não presumir que está começando do zero e
/// refazer o que já estava provado). PURA — testável sem processo.
/// **Entrada:** nº do relançamento. **Saída:** o prompt.
pub fn prompt_retomada(tentativa: u32) -> String {
    format!(
        "RETOMADA AUTOMÁTICA do overdev (relançamento {tentativa}). O agente anterior encerrou \
         com item de máquina AINDA ABERTO — provavelmente contexto estourado, compactação ou \
         crash, não conclusão. NÃO recomece do zero: releia `.schematize/overdev/CHECKLIST.md`, \
         `PLAN.md` e `DECISOES.md`, confie no que já está `- [x]` (foi provado) e retome do \
         PRIMEIRO `- [ ]` aberto. Se algum `- [x]` não se provar hoje, reabra-o. Siga a \
         disciplina do overdev: fecha item só com prova, não pare enquanto houver `- [ ]`."
    )
}

/// Uma decisão de ronda — separada do laço pra ser testável sem `/proc` nem sleep.
#[derive(Debug, PartialEq, Eq)]
pub enum Ronda {
    /// Nada a fazer: não há mais trabalho de máquina (ou o run foi parado).
    Encerrar,
    /// Há trabalho e há agente — segue vigiando.
    Seguir,
    /// Há trabalho e NÃO há agente há rondas suficientes — relançar.
    Relancar,
}

/// Decide o que fazer numa ronda, a partir dos fatos já colhidos.
///
/// O quê: a regra do supervisor, isolada do I/O. Onde: [`supervise`] a cada ronda.
/// **Entrada:** se há trabalho de máquina, se há agente vivo, e há quantas rondas seguidas
/// sem agente. **Saída:** a ação. **Efeitos:** nenhum.
pub fn decidir(trabalho: bool, agente: bool, rondas_sem_agente: u32) -> Ronda {
    if !trabalho {
        return Ronda::Encerrar;
    }
    if agente {
        return Ronda::Seguir;
    }
    if rondas_sem_agente >= RONDAS_ATE_RELANCAR {
        Ronda::Relancar
    } else {
        Ronda::Seguir
    }
}

/// Registra um relançamento no archive do projeto (best-effort).
fn registrar(projeto: &Path, linha: &str) {
    let arq = crate::paths::overdev_dir_at(projeto).join("supervisor.log");
    let _ = std::fs::create_dir_all(arq.parent().unwrap_or(Path::new(".")));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&arq) {
        use std::io::Write;
        let _ = writeln!(f, "[{}] {linha}", crate::util::now_unix());
    }
}

/// O laço do supervisor. Vigia até o trabalho de máquina acabar ou o teto estourar.
///
/// O quê: a cada [`RONDA_SEGS`] decide por [`decidir`]; relançando, dispara o `claude` num
/// terminal com [`prompt_retomada`]. Onde: `schematize overdev supervise`.
/// **Entrada:** raiz do projeto; teto de relançamentos.
/// **Saída:** quantos relançamentos foram feitos.
/// **Efeitos:** abre terminais, escreve `supervisor.log`, dorme entre rondas.
pub fn supervise(projeto: &Path, max: u32) -> u32 {
    let projeto: PathBuf = projeto.canonicalize().unwrap_or_else(|_| projeto.to_path_buf());
    let mut relancamentos = 0u32;
    let mut sem_agente = 0u32;
    registrar(&projeto, "supervisor iniciado");
    loop {
        let trabalho = tem_trabalho_de_maquina(&projeto);
        let agente = agente_vivo_em(&projeto);
        sem_agente = if agente { 0 } else { sem_agente + 1 };
        match decidir(trabalho, agente, sem_agente) {
            Ronda::Encerrar => {
                registrar(&projeto, "sem trabalho de máquina aberto — supervisor encerrado");
                return relancamentos;
            }
            Ronda::Seguir => {}
            Ronda::Relancar => {
                if relancamentos >= max {
                    registrar(&projeto, &format!("TETO de {max} relançamentos atingido — encerrando. O agente morre repetidamente; isto não é ausência de agente, é outro problema."));
                    return relancamentos;
                }
                relancamentos += 1;
                sem_agente = 0;
                let p = prompt_retomada(relancamentos);
                match agentrun::launch_prompt_in_terminal(&projeto, &p) {
                    Ok(term) => {
                        registrar(&projeto, &format!("relançamento {relancamentos} em {term}"))
                    }
                    Err(e) => {
                        registrar(&projeto, &format!("relançamento {relancamentos} FALHOU: {e}"))
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(RONDA_SEGS));
    }
}

/// Já existe um supervisor vigiando este projeto?
///
/// O quê: varre `/proc` atrás de um `schematize … overdev supervise` cujo cwd é o projeto.
/// Onde: [`garantir_supervisor`], pra não subir dois. **Efeitos:** só leitura de `/proc`.
pub fn supervisor_vivo_em(projeto: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let linha = String::from_utf8_lossy(&raw).replace('\0', " ");
        if !(linha.contains("overdev") && linha.contains("supervise")) {
            continue;
        }
        if let Ok(cwd) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
            if cwd == projeto {
                return true;
            }
        }
    }
    false
}

/// Sobe um supervisor DESTACADO pra este projeto, se ainda não houver um.
///
/// O quê: spawna `schematize overdev supervise` com o cwd no projeto, desacoplado do
/// processo que chamou (sobrevive ao fechamento da GUI/terminal).
/// Onde: [`crate::agentrun::launch_in_terminal`], logo após lançar o agente.
///
/// Por que automático: depender de o usuário lembrar de subir a rede de segurança é o
/// mesmo que não ter rede. Idempotente — dois supervisores no mesmo projeto se
/// atrapalhariam relançando em dobro.
///
/// **Entrada:** raiz do projeto. **Saída:** `true` se subiu agora; `false` se já havia um
/// ou se não deu (best-effort — falhar aqui nunca pode impedir o run de começar).
/// **Efeitos:** cria processo.
pub fn garantir_supervisor(projeto: &Path) -> bool {
    let projeto = projeto.canonicalize().unwrap_or_else(|_| projeto.to_path_buf());
    if supervisor_vivo_em(&projeto) {
        return false;
    }
    let exe = crate::util::self_exe();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("overdev")
        .arg("supervise")
        .current_dir(&projeto)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // sobrevive ao fechamento de quem chamou
    }
    cmd.spawn().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O QUE: sem trabalho de máquina, encerra — mesmo sem agente. É o que impede o
    /// supervisor de ficar relançando eternamente um run já concluído.
    #[test]
    fn sem_trabalho_encerra() {
        assert_eq!(decidir(false, false, 99), Ronda::Encerrar);
        assert_eq!(decidir(false, true, 0), Ronda::Encerrar);
    }

    /// O QUE: com agente vivo, nunca relança — não duplica claude no mesmo projeto.
    #[test]
    fn com_agente_vivo_nao_relanca() {
        assert_eq!(decidir(true, true, 99), Ronda::Seguir);
    }

    /// O QUE: a ausência de agente precisa PERSISTIR pra virar relançamento. Uma ronda só
    /// pode ser o terminal ainda subindo — relançar ali criaria dois claudes no projeto.
    #[test]
    fn ausencia_precisa_persistir() {
        assert_eq!(decidir(true, false, 1), Ronda::Seguir);
        assert_eq!(decidir(true, false, RONDAS_ATE_RELANCAR), Ronda::Relancar);
    }

    /// O QUE: o prompt de retomada diz que NÃO é começo do zero — sem isso o agente
    /// relançado tende a refazer o que já estava provado.
    #[test]
    fn retomada_avisa_que_nao_e_do_zero() {
        let p = prompt_retomada(3);
        assert!(p.contains("NÃO recomece do zero"), "{p}");
        assert!(p.contains("relançamento 3"), "{p}");
    }

    /// O QUE: `tem_trabalho_de_maquina` lê o control-plane REAL de um projeto — é o fato
    /// que decide relançar ou encerrar, então precisa ser exercitado contra disco, não só
    /// contra a regra pura.
    ///
    /// Cobre os três estados que importam: run ativo com `- [ ]` (há trabalho), run ativo
    /// com tudo fechado (não há), e run PARADO com item aberto (não há — `overdev stop` é
    /// a saída do usuário; se isto falhar, parar vira impossível e o supervisor relança
    /// pra sempre).
    #[test]
    fn le_o_trabalho_do_projeto_no_disco() {
        let base = std::env::temp_dir().join(format!("sz-sup-{}", std::process::id()));
        let od = base.join(".schematize").join("overdev");
        std::fs::create_dir_all(&od).expect("criar control-plane");

        let escreve = |mode: &str, checklist: &str| {
            std::fs::write(
                od.join("state.json"),
                format!(r#"{{"mode":"{mode}","max_iters":200,"objetivo":"t","started":0}}"#),
            )
            .unwrap();
            std::fs::write(od.join("CHECKLIST.md"), checklist).unwrap();
        };

        escreve("active", "- [ ] fazer algo\n- [x] feito\n");
        assert!(tem_trabalho_de_maquina(&base), "ativo com `- [ ]` aberto TEM trabalho");

        escreve("active", "- [x] feito\n- [H ] humano\n- [~] parkeado\n");
        assert!(
            !tem_trabalho_de_maquina(&base),
            "humano e on-hold NÃO são trabalho de máquina — relançar aqui seria laço infinito"
        );

        escreve("stopped", "- [ ] fazer algo\n");
        assert!(
            !tem_trabalho_de_maquina(&base),
            "run parado não tem trabalho: `overdev stop` precisa ser uma saída de verdade"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
