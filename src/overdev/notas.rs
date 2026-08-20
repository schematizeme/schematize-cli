//! NOTAS, correções e o fechamento de item HUMANO (o que só a pessoa fecha), mais
//! o parking de pergunta (`- [~]`) que é a saída honesta pro agente travado.

use super::*;

/// Parkeia uma pergunta: registra no txt da base e marca o item como on-hold.
pub fn park(item_substr: &str, pergunta: &str) -> Result<(), String> {
    let pergunta = pergunta.trim();
    if pergunta.is_empty() {
        return Err("pergunta vazia".into());
    }
    // Id do VÍNCULO: é ele que, mais tarde, faz a resposta saber qual item de máquina
    // liberar. Sem isso a pergunta é um bilhete solto e o item fica on-hold pra sempre.
    let id = super::caixa::id_curto();

    let cl = checklist();
    super::trava::com_trava(&cl, || {
        let s = fs::read_to_string(&cl).map_err(|e| e.to_string())?;
        let out = super::resposta::parkear_str(&s, item_substr, pergunta, &id)?;
        super::trava::escreve_atomico(&cl, &out)
    })?;

    // O txt na base do projeto continua sendo escrito: é onde o agente foi instruído a
    // olhar e onde a pessoa costuma procurar. Agora com o id, pra casar com o checklist.
    let mut q = fs::read_to_string(QUESTIONS_FILE).unwrap_or_default();
    q.push_str(&format!(
        "[{}] ({id}) item: {item_substr}\n  pergunta: {pergunta}\n\n",
        util::now_unix()
    ));
    let _ = fs::write(QUESTIONS_FILE, q);

    println!("pergunta parkeada como subtask do item ({id}); o item ficou on-hold.");
    println!("responder libera a máquina:  overflow overdev answer 1 \"...\"");
    println!("recusar cancela o item:      overflow overdev refuse 1 \"...\"");
    Ok(())
}

/// Resolve um item humano: responde (libera a máquina) ou recusa (cancela).
///
/// Todo o ciclo ler-modificar-escrever sob a MESMA trava — o agente pode estar
/// escrevendo no checklist agora, e o alvo é posicional.
pub fn resolver(alvo: super::resposta::Alvo, acao: super::resposta::Acao, texto: &str) -> Result<(), String> {
    let cl = checklist();
    let r = super::trava::com_trava(&cl, || {
        let s = fs::read_to_string(&cl).map_err(|e| e.to_string())?;
        let r = super::resposta::resolver_str(&s, &alvo, acao, texto)?;
        super::trava::escreve_atomico(&cl, &r.texto)?;
        Ok(r)
    })?;

    // A decisão também vai pro registro durável do projeto. O checklist é operacional
    // (some quando o item fecha); DECISOES.md é a memória de POR QUE se decidiu assim.
    let rotulo = if acao == super::resposta::Acao::Responder { "RESPOSTA" } else { "RECUSA" };
    let bloco = format!("\n## {rotulo}: {}\n\n{texto}\n", r.item);
    let dec = dir().join("DECISOES.md");
    let mut atual = fs::read_to_string(&dec).unwrap_or_default();
    atual.push_str(&bloco);
    let _ = super::trava::escreve_atomico(&dec, &atual);

    match (&r.vinculado, acao) {
        (Some(m), super::resposta::Acao::Responder) => {
            println!("respondido: {}", r.item);
            println!("→ liberado pra máquina: {m}");
        }
        (Some(m), super::resposta::Acao::Recusar) => {
            println!("recusado: {}", r.item);
            println!("→ cancelado: {m}");
        }
        (None, _) => println!("{}: {}", if acao == super::resposta::Acao::Responder { "respondido" } else { "recusado" }, r.item),
    }
    let _ = crate::overdevdb::snapshot(Path::new("."));
    Ok(())
}

/// Marca o primeiro `- [ ]` que contém `substr` como `- [~]` (on-hold).
pub fn hold(substr: &str) -> Result<(), String> {
    // A LEITURA fica dentro da trava, junto da escrita.
    //
    // Isto é um ciclo ler-modificar-escrever sobre um arquivo que outro processo (o
    // agente do overdev, a GUI, outra sessão) pode estar reescrevendo agora. Ler fora
    // da trava é o bug clássico: você decide em cima de um estado que já mudou e grava
    // por cima do trabalho alheio, sem erro nenhum. Ver `overdev::trava`.
    let cl = checklist();
    super::trava::com_trava(&cl, || {
        let s = fs::read_to_string(&cl).map_err(|e| e.to_string())?;
        let mut done = false;
        let out: Vec<String> = s
            .lines()
            .map(|l| {
                if !done && l.trim_start().starts_with("- [ ]") && l.contains(substr) {
                    done = true;
                    l.replacen("- [ ]", "- [~]", 1)
                } else {
                    l.to_string()
                }
            })
            .collect();
        if !done {
            // Nada casou: sai SEM escrever. Reescrever o arquivo idêntico só criaria
            // uma janela de risco de graça.
            return Err(format!("nenhum item aberto contém '{substr}'"));
        }
        super::trava::escreve_atomico(&cl, &out.join("\n"))
    })
}

/// Fecha o primeiro `- [H ]` (humano aberto) → `- [H x]` — PURO, testável.
/// Casa por `substr` (contém) OU por `index` (1-based entre os humanos abertos).
/// Retorna (novo conteúdo, texto do item fechado).
pub(crate) fn mark_human_str(s: &str, substr: Option<&str>, index: Option<usize>) -> Result<(String, String), String> {
    let mut seen = 0usize; // contador de humanos abertos vistos (1-based)
    let mut hit: Option<String> = None;
    let out: Vec<String> = s
        .lines()
        .map(|l| {
            if hit.is_none() && l.trim_start().starts_with("- [H ]") {
                seen += 1;
                let matches = match (substr, index) {
                    (_, Some(n)) => seen == n,
                    (Some(sub), None) => l.contains(sub),
                    (None, None) => false,
                };
                if matches {
                    hit = Some(l.trim().to_string());
                    return l.replacen("- [H ]", "- [H x]", 1);
                }
            }
            l.to_string()
        })
        .collect();
    match hit {
        Some(txt) => Ok((out.join("\n"), txt)),
        None => Err(match (substr, index) {
            (_, Some(n)) => format!("não há {n}º item humano aberto (- [H ])"),
            (Some(sub), None) => format!("nenhum item humano aberto contém '{sub}'"),
            (None, None) => "informe o texto do item ou --done <n>".to_string(),
        }),
    }
}

/// CLI: o HUMANO fecha um item `- [H ]` → `- [H x]` (pela CLI ou GUI).
/// `substr` casa pelo texto; `index` (--done N) casa pela posição entre os humanos abertos.
pub fn human_done(substr: Option<&str>, index: Option<usize>) -> Result<(), String> {
    // Ler e escrever sob a MESMA trava — ver a nota em `hold`. O índice do item humano
    // é posicional: decidir com uma leitura velha fecharia o item errado.
    let cl = checklist();
    let txt = super::trava::com_trava(&cl, || {
        let s = fs::read_to_string(&cl).map_err(|e| e.to_string())?;
        let (out, txt) = mark_human_str(&s, substr, index)?;
        super::trava::escreve_atomico(&cl, &out)?;
        Ok(txt)
    })?;
    // Versiona a mudança no DB local (best-effort). Fora da trava: é I/O em outro
    // arquivo e não pode segurar o checklist.
    let _ = crate::overdevdb::snapshot(Path::new("."));
    println!("item humano fechado: {txt}");
    Ok(())
}

pub(crate) fn notas_file(root: &Path) -> PathBuf {
    crate::paths::overdev_dir_at(root).join("NOTAS.md")
}

/// Formata um bloco de nota (PURO). `kind`: "correcao" (prompt de correção do
/// overdev), "task" (ponto específico por task) ou livre; `texto` é o conteúdo.
pub(crate) fn note_block(kind: &str, texto: &str) -> String {
    let label = match kind {
        "correcao" | "correction" => "PROMPT DE CORREÇÃO",
        "task" => "PONTO POR TASK",
        other => other,
    };
    format!("## [{}] {}\n\n{}\n\n", util::now_unix(), label, texto.trim())
}

/// Anexa uma nota do humano em `<root>/.overdev/NOTAS.md` (cria se preciso).
pub fn add_note(root: &Path, kind: &str, texto: &str) -> Result<(), String> {
    let f = notas_file(root);
    if let Some(d) = f.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let mut cur = fs::read_to_string(&f).unwrap_or_else(|_| "# OVERDEV — NOTAS do humano\n\n".to_string());
    cur.push_str(&note_block(kind, texto));
    fs::write(&f, cur).map_err(|e| e.to_string())?;
    // Versiona a nota no DB local (best-effort).
    let _ = crate::overdevdb::snapshot(root);
    Ok(())
}

/// Lê as notas do humano (vazio se não houver) — consumível pela GUI.
pub fn read_notes(root: &Path) -> String {
    fs::read_to_string(notas_file(root)).unwrap_or_default()
}

/// CLI: `schematize overdev note "<texto>" [--kind correcao|task]`.
pub fn note(kind: &str, texto: &str) -> Result<(), String> {
    add_note(Path::new("."), kind, texto)?;
    println!("nota registrada em .schematize/overdev/NOTAS.md ({kind}).");
    Ok(())
}
