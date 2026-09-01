//! Prompts e a heurística de NUDGE do agente acoplado.
//!
//! O quê: as strings em linguagem natural que o app manda ao `claude` (overdev, reindex,
//! afazeres do archive) e as duas peças PURAS que decidem quando cutucar um agente ocioso.
//! Onde: consumido pelo `super` (sessão acoplada) e pelo `lancador`.
//!
//! Separado porque é o único pedaço do `agentrun` sem I/O: muda por razão de PRODUTO (o
//! texto que o agente lê), não por razão de plataforma.

use super::*;

/// Quantos itens abertos, no máximo, entram na mensagem de nudge.
pub(crate) const NUDGE_ITEMS: usize = 8;

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
    let alvo = if o.is_empty() { String::new() } else { format!(" O objetivo é: {o}.") };
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
