//! Os subcomandos da FILA DE PERGUNTAS — `overdev ask|questions|reply|review`.
//!
//! **Onde:** despachados por `main.rs`. O modelo e as invariantes estão em
//! [`schematize::overdev::pergunta`]; aqui está só a borda de linha de comando.
//!
//! **Por que uma borda separada do `cli/overdev.rs`:** aquele arquivo já tem 211 linhas e trata
//! do LAÇO (rodar, dividir, snapshot). A fila é outra superfície — quem a usa é o agente para
//! perguntar e a janela para responder.

use schematize::overdev::pergunta::{self, Kind, Opcao, Pergunta, Resposta};
use std::path::PathBuf;

/// **O quê:** o diretório do projeto atual.
///
/// **Onde:** todos os subcomandos daqui. Erro claro em vez de `unwrap`: um cwd inacessível
/// acontece (diretório apagado sob o processo) e o piso 4 proíbe calar.
fn raiz() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("cwd inacessível: {e}"))
}

/// **O quê:** epoch de agora, em segundos.
///
/// **Onde:** criação e resposta. Antes de 1970 não existe para este efeito; `unwrap_or_default`
/// devolve 0 e o log mostra `0`, que é visivelmente errado — melhor que panicar num relógio torto.
fn agora() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// **O quê:** o `Kind` a partir do texto da flag, ou erro que LISTA os válidos.
///
/// **Onde:** [`ask`]. Listar os válidos no erro é o que evita a pessoa adivinhar — e o conjunto
/// é fechado justamente porque cada um desenha um controle diferente na janela.
fn kind_de(s: &str) -> Result<Kind, String> {
    match s {
        "aprovar" => Ok(Kind::Aprovar),
        "escolha" => Ok(Kind::Escolha),
        "multipla" => Ok(Kind::Multipla),
        "sim_nao_outro" => Ok(Kind::SimNaoOutro),
        "livre" => Ok(Kind::Livre),
        outro => Err(format!(
            "kind `{outro}` não existe. Os cinco: aprovar, escolha, multipla, sim_nao_outro, livre"
        )),
    }
}

/// **O quê:** `id:rótulo` ou `id:rótulo:detalhe` → [`Opcao`].
///
/// **Onde:** [`ask`]. Função pura, testada — é onde um `--opcao` mal escrito tem de virar erro
/// legível em vez de uma opção com rótulo vazio que a janela desenha como botão sem texto.
pub fn parse_opcao(s: &str) -> Result<Opcao, String> {
    let mut partes = s.splitn(3, ':');
    let id = partes.next().unwrap_or("").trim();
    let rotulo = partes.next().unwrap_or("").trim();
    let detalhe = partes.next().unwrap_or("").trim();
    if id.is_empty() || rotulo.is_empty() {
        return Err(format!(
            "`--opcao {s}` não tem a forma `id:rótulo` (ou `id:rótulo:detalhe`). \
             O id é o que a resposta grava; o rótulo é o texto do botão."
        ));
    }
    Ok(Opcao { id: id.to_string(), rotulo: rotulo.to_string(), detalhe: detalhe.to_string() })
}

/// **O quê:** acha a pergunta por id exato ou por trecho do título.
///
/// **Onde:** [`reply`] e [`review`]. Recusa ambiguidade em vez de escolher a primeira: resolver
/// a pergunta errada é pior que pedir para a pessoa ser específica.
fn achar(raiz: &std::path::Path, alvo: &str) -> Result<Pergunta, String> {
    let (todas, ruins) = pergunta::listar(raiz);
    for r in &ruins {
        eprintln!("aviso: {r}");
    }
    if let Some(p) = todas.iter().find(|p| p.id == alvo) {
        return Ok(p.clone());
    }
    let baixo = alvo.to_lowercase();
    let casam: Vec<&Pergunta> =
        todas.iter().filter(|p| p.titulo.to_lowercase().contains(&baixo)).collect();
    match casam.as_slice() {
        [um] => Ok((*um).clone()),
        [] => Err(format!("nenhuma pergunta com id ou título contendo `{alvo}`")),
        muitas => Err(format!(
            "`{alvo}` casa com {} perguntas — use o id. As candidatas:\n{}",
            muitas.len(),
            muitas
                .iter()
                .map(|p| format!("  {} — {}", p.id, p.titulo))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

/// `schematize overdev ask` — o agente enfileira um quiz.
pub fn ask(
    titulo: &str,
    kind: &str,
    opcoes: &[String],
    contexto: &str,
    auto: &str,
    linka: &[String],
) -> Result<(), String> {
    let raiz = raiz()?;
    let k = kind_de(kind)?;
    let opcoes: Vec<Opcao> =
        opcoes.iter().map(|s| parse_opcao(s)).collect::<Result<Vec<_>, _>>()?;
    let p = Pergunta {
        id: pergunta::novo_id(&raiz, agora()),
        criada_em: agora(),
        fase: String::new(),
        titulo: titulo.trim().to_string(),
        contexto: contexto.trim().to_string(),
        kind: k,
        opcoes,
        auto: auto.trim().to_string(),
        linka: linka.to_vec(),
        resposta: None,
    };
    let arq = pergunta::gravar(&raiz, &p)?;
    println!("pergunta enfileirada: {} — {}", p.id, p.titulo);
    println!("  {}", arq.display());
    if !p.linka.is_empty() {
        println!("  destrava: {}", p.linka.join(", "));
    }
    println!("A pessoa responde pela janela (aba Overdev) ou por `schematize overdev reply`.");
    Ok(())
}

/// `schematize overdev questions [--json]` — a fila.
///
/// **Três grupos, e o do meio é o que existe por pedido explícito:** aberta, **respondida
/// aguardando revisão da máquina**, e revisada. Juntar os dois últimos esconderia exatamente o
/// estado que o usuário pediu para existir.
pub fn questions(json: bool) -> Result<(), String> {
    let raiz = raiz()?;
    let (todas, ruins) = pergunta::listar(&raiz);
    if json {
        // Saída de MÁQUINA: a janela lê isto. Um objeto com as três listas, nunca prosa.
        let v = serde_json::json!({
            "abertas": todas.iter().filter(|p| p.aberta()).collect::<Vec<_>>(),
            "aguardando_revisao": todas.iter().filter(|p| p.aguarda_revisao()).collect::<Vec<_>>(),
            "revisadas": todas
                .iter()
                .filter(|p| p.resposta.as_ref().is_some_and(|r| r.revisada))
                .collect::<Vec<_>>(),
            "ilegiveis": ruins,
        });
        println!("{}", serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?);
        return Ok(());
    }
    for r in &ruins {
        eprintln!("ILEGÍVEL: {r}");
    }
    let (abertas, resto): (Vec<_>, Vec<_>) = todas.iter().partition(|p| p.aberta());
    println!("ABERTAS ({})", abertas.len());
    for p in &abertas {
        println!("  {} [{}] {}", p.id, rotulo_kind(p.kind), p.titulo);
        for o in &p.opcoes {
            let marca = if o.id == p.auto { "*" } else { " " };
            println!("      {marca} {}  {}", o.id, o.rotulo);
        }
        if !p.auto.is_empty() {
            println!("      (* = sugestão do agente; ninguém a aplicou)");
        }
    }
    let (revisar, revisadas): (Vec<&&Pergunta>, Vec<&&Pergunta>) =
        resto.iter().partition(|p| p.aguarda_revisao());
    println!("\nRESPONDIDAS, AGUARDANDO REVISÃO DA MÁQUINA ({})", revisar.len());
    for p in &revisar {
        println!("  {} — {}\n      resposta: {}", p.id, p.titulo, p.resposta_em_texto());
    }
    println!("\nREVISADAS ({})", revisadas.len());
    for p in &revisadas {
        let rev = p.resposta.as_ref().map(|r| r.revisao.clone()).unwrap_or_default();
        println!("  {} — {}\n      revisão: {rev}", p.id, p.titulo);
    }
    Ok(())
}

fn rotulo_kind(k: Kind) -> &'static str {
    match k {
        Kind::Aprovar => "aprovar/negar",
        Kind::Escolha => "uma de N",
        Kind::Multipla => "N de N",
        Kind::SimNaoOutro => "sim/não/outro",
        Kind::Livre => "texto livre",
    }
}

/// `schematize overdev reply <alvo> [--escolha id]… [--texto "…"]`
///
/// **Grava `revisada: false` SEMPRE.** É o ponto do desenho: responder não libera nada; quem
/// libera é a revisão da máquina. Um caminho que já marcasse revisado aqui seria a rota de fuga.
pub fn reply(alvo: &str, escolhas: &[String], texto: &str) -> Result<(), String> {
    let raiz = raiz()?;
    let mut p = achar(&raiz, alvo)?;
    if escolhas.is_empty() && texto.trim().is_empty() {
        return Err(
            "resposta vazia não é resposta — passe `--escolha <id>` e/ou `--texto \"…\"`".into()
        );
    }
    if let Some(r) = &p.resposta {
        println!(
            "aviso: esta pergunta já tinha resposta ({}), e ela está sendo substituída.",
            r.em
        );
    }
    p.resposta = Some(Resposta {
        em: agora(),
        por: "humano".into(),
        escolhas: escolhas.to_vec(),
        texto: texto.trim().to_string(),
        revisada: false,
        revisao: String::new(),
    });
    // A validação em `gravar` é o que recusa um `--escolha` que não é opção declarada.
    pergunta::gravar(&raiz, &p)?;
    println!("respondida: {} — {}", p.id, p.resposta_em_texto());
    println!("AGUARDANDO REVISÃO DA MÁQUINA — o item vinculado ainda não foi liberado.");
    if !p.linka.is_empty() {
        println!("  vai destravar: {}", p.linka.join(", "));
    }
    Ok(())
}

/// `schematize overdev review <alvo> "<conclusão>"` — a máquina leu a resposta.
pub fn review(alvo: &str, texto: &str) -> Result<(), String> {
    let raiz = raiz()?;
    let mut p = achar(&raiz, alvo)?;
    let Some(r) = p.resposta.as_mut() else {
        return Err(format!("`{}` ainda não foi respondida — não há o que revisar", p.id));
    };
    if texto.trim().is_empty() {
        return Err(
            "a revisão não pode ser vazia: ela é o registro do que a máquina concluiu".into()
        );
    }
    r.revisada = true;
    r.revisao = texto.trim().to_string();
    pergunta::gravar(&raiz, &p)?;
    println!("revisada: {} — {}", p.id, p.titulo);
    println!("  resposta: {}", p.resposta_em_texto());
    println!("  conclusão: {texto}");
    if !p.linka.is_empty() {
        println!("  LIBERADO: {}", p.linka.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_opcao_aceita_as_duas_formas() {
        let o = parse_opcao("propria:Tela própria").expect("forma curta");
        assert_eq!(
            (o.id.as_str(), o.rotulo.as_str(), o.detalhe.as_str()),
            ("propria", "Tela própria", "")
        );
        let o = parse_opcao("sec:Lista secundária:fica colapsada").expect("com detalhe");
        assert_eq!(o.detalhe, "fica colapsada");
        // Dois-pontos no DETALHE não parte a opção em três — `splitn(3)` para no terceiro.
        let o = parse_opcao("a:B:c: d: e").expect("detalhe com dois-pontos");
        assert_eq!(o.detalhe, "c: d: e");
    }

    /// Opção sem rótulo desenharia botão sem texto na janela. Erro legível, não opção vazia.
    #[test]
    fn parse_opcao_recusa_forma_errada() {
        for ruim in ["", "so-id", "so-id:", ":so-rotulo", "  :  "] {
            let e = parse_opcao(ruim).expect_err("{ruim:?} tinha de reprovar");
            assert!(e.contains("id:rótulo"), "a mensagem tem de ensinar a forma: {e}");
        }
    }

    /// `kind` inválido LISTA os válidos — senão a pessoa adivinha.
    #[test]
    fn kind_invalido_lista_os_validos() {
        let e = kind_de("aprovacao").expect_err("não existe");
        for k in ["aprovar", "escolha", "multipla", "sim_nao_outro", "livre"] {
            assert!(e.contains(k), "o erro tem de listar `{k}`: {e}");
        }
        assert_eq!(kind_de("escolha").unwrap(), Kind::Escolha);
    }
}
