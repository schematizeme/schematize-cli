//! RESPONDER e RECUSAR itens humanos — e o que isso faz com o item de máquina.
//!
//! ## O que faltava
//!
//! Um item humano só tinha dois destinos: aberto ou feito. Mas boa parte deles não é
//! trabalho — é **decisão**. O agente trava numa dúvida de ADR, parkeia a pergunta e
//! marca o item de máquina `- [~]`; a pessoa volta, decide, e... não havia como
//! registrar isso. Fechar como "feito" seria mentira (ninguém fez nada) e, pior, não
//! destravava o item de máquina: ele seguia on-hold pra sempre.
//!
//! O caso concreto que motivou: "preciso que você faça o deploy". A resposta certa
//! muitas vezes é "pode dar o deploy você mesmo" — não fiz nada, autorizei. Isso não
//! é "feito", é **respondido**, e tem de liberar a máquina.
//!
//! ## Os estados
//!
//! | marcador   | significado                                              |
//! |------------|----------------------------------------------------------|
//! | `- [H ]`   | humano aberto                                            |
//! | `- [H x]`  | humano FEZ                                               |
//! | `- [H r]`  | humano RESPONDEU — libera o item de máquina vinculado     |
//! | `- [H -]`  | humano RECUSOU — cancela o item de máquina vinculado      |
//! | `- [-]`    | item de máquina CANCELADO (consequência de uma recusa)    |
//!
//! Recusar existe porque nem tudo que a máquina pede é cabível, e a única saída hoje
//! era fingir que foi feito. Item recusado sai da conta de trabalho SEM entrar na de
//! feito — mentir no progresso seria trocar um problema por outro.
//!
//! ## O vínculo
//!
//! Um comentário HTML com id compartilhado nas duas linhas (`<!-- ovf:q:<id> -->`),
//! e o item humano INDENTADO sob o de máquina. Assim o vínculo é visível no Markdown
//! cru, sobrevive a edição manual e não exige um índice à parte que dessincronizaria.
//! A indentação também é o que faz a GUI e qualquer leitor renderizarem como subtask.

/// Prefixo do marcador de vínculo pergunta↔item.
pub const MARCA_Q: &str = "ovf:q:";

/// Como o chamador aponta qual item humano quer resolver.
#[derive(Debug, Clone)]
pub enum Alvo {
    /// Posição 1-based entre os humanos ABERTOS (o mesmo índice que a UI mostra).
    Indice(usize),
    /// Trecho do texto do item.
    Texto(String),
}

/// O que fazer com o item humano.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acao {
    /// Respondeu: registra a resposta e LIBERA o item de máquina vinculado.
    Responder,
    /// Recusou: registra o motivo e CANCELA o item de máquina vinculado.
    Recusar,
}

impl Acao {
    fn marcador(self) -> &'static str {
        match self {
            Acao::Responder => "- [H r]",
            Acao::Recusar => "- [H -]",
        }
    }
    fn rotulo(self) -> &'static str {
        match self {
            Acao::Responder => "resposta",
            Acao::Recusar => "recusado",
        }
    }
}

/// Resultado de resolver um item humano.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolucao {
    /// O checklist inteiro, já modificado.
    pub texto: String,
    /// O texto do item humano resolvido (pra ecoar pro usuário).
    pub item: String,
    /// O item de máquina vinculado, se havia — já liberado ou cancelado.
    pub vinculado: Option<String>,
}

/// Extrai o id de vínculo de uma linha, se houver.
fn id_de(l: &str) -> Option<String> {
    let i = l.find(MARCA_Q)? + MARCA_Q.len();
    let resto = &l[i..];
    let fim = resto.find([' ', '-', '>'])?;
    let id = &resto[..fim];
    (!id.is_empty()).then(|| id.to_string())
}

/// O texto útil de uma linha de item: sem o marcador de estado e sem o comentário.
fn texto_do_item(l: &str) -> String {
    let t = l.trim();
    let sem_marca = t.split_once(']').map(|x| x.1).unwrap_or(t);
    let sem_comentario = sem_marca.split("<!--").next().unwrap_or(sem_marca);
    sem_comentario.trim().to_string()
}

/// A linha é um item humano ABERTO?
fn humano_aberto(l: &str) -> bool {
    l.trim_start().starts_with("- [H ]")
}

/// Resolve um item humano — PURA: recebe e devolve o texto do checklist.
///
/// Pura porque a decisão é a parte que pode errar (qual item casou, qual item de
/// máquina liberar) e é a que precisa de teste. A I/O fica com quem chama, sob trava.
pub fn resolver_str(s: &str, alvo: &Alvo, acao: Acao, texto: &str) -> Result<Resolucao, String> {
    let texto = texto.trim();
    if texto.is_empty() {
        return Err(match acao {
            Acao::Responder => "resposta vazia — responder é justamente registrar a decisão".into(),
            Acao::Recusar => "motivo vazio — recusar sem dizer por quê não ajuda ninguém".into(),
        });
    }
    let linhas: Vec<&str> = s.lines().collect();

    // 1) Acha o item humano aberto que casa. Índice conta SÓ entre os abertos, que é
    //    o que a UI numera — usar a posição absoluta fecharia o item errado.
    let mut aberto_n = 0usize;
    let alvo_idx = linhas
        .iter()
        .position(|l| {
            if !humano_aberto(l) {
                return false;
            }
            aberto_n += 1;
            match alvo {
                Alvo::Indice(n) => aberto_n == *n,
                Alvo::Texto(t) => l.contains(t.as_str()),
            }
        })
        .ok_or_else(|| match alvo {
            Alvo::Indice(n) => format!("não há um {n}º item humano aberto"),
            Alvo::Texto(t) => format!("nenhum item humano aberto contém '{t}'"),
        })?;

    let item = texto_do_item(linhas[alvo_idx]);
    let vinculo = id_de(linhas[alvo_idx]);
    let indent: String = linhas[alvo_idx].chars().take_while(|c| c.is_whitespace()).collect();

    // 2) Reescreve. Preserva o comentário de vínculo — ele é o que mantém o par legível.
    let mut saida: Vec<String> = Vec::with_capacity(linhas.len() + 1);
    let mut vinculado: Option<String> = None;
    for (i, l) in linhas.iter().enumerate() {
        if i == alvo_idx {
            saida.push(l.replacen("- [H ]", acao.marcador(), 1));
            // A resposta entra como filha do item, no arquivo que o agente lê. Guardar
            // só num arquivo à parte deixaria a decisão longe de onde ela é aplicada.
            saida.push(format!("{indent}  - {}: {texto}", acao.rotulo()));
            continue;
        }
        // 3) O item de MÁQUINA vinculado muda de estado junto.
        let casa_vinculo = vinculo.as_deref().is_some_and(|id| id_de(l).as_deref() == Some(id));
        let t = l.trim_start();
        if casa_vinculo && t.starts_with("- [~]") {
            vinculado = Some(texto_do_item(l));
            saida.push(match acao {
                // Respondida a dúvida, a máquina volta a poder trabalhar nele.
                Acao::Responder => l.replacen("- [~]", "- [ ]", 1),
                // Recusado: cancelar, não liberar. Liberar faria o agente retomar
                // exatamente a tarefa que a pessoa acabou de rejeitar.
                Acao::Recusar => l.replacen("- [~]", "- [-]", 1),
            });
            continue;
        }
        saida.push(l.to_string());
    }
    let mut texto_final = saida.join("\n");
    if s.ends_with('\n') && !texto_final.ends_with('\n') {
        texto_final.push('\n');
    }
    Ok(Resolucao { texto: texto_final, item, vinculado })
}

/// Monta o par vinculado (item de máquina on-hold + pergunta humana indentada) — PURA.
///
/// É o que transforma "pergunta solta num txt" em "subtask do item que ela trava":
/// as duas linhas passam a carregar o mesmo id, e a indentação mostra a dependência.
pub fn parkear_str(s: &str, item_substr: &str, pergunta: &str, id: &str) -> Result<String, String> {
    let mut achou = false;
    let mut saida: Vec<String> = Vec::new();
    for l in s.lines() {
        if !achou && l.trim_start().starts_with("- [ ]") && l.contains(item_substr) {
            achou = true;
            let indent: String = l.chars().take_while(|c| c.is_whitespace()).collect();
            saida.push(format!("{} <!-- {MARCA_Q}{id} -->", l.replacen("- [ ]", "- [~]", 1)));
            saida.push(format!("{indent}  - [H ] {pergunta} <!-- {MARCA_Q}{id} -->"));
            continue;
        }
        saida.push(l.to_string());
    }
    if !achou {
        return Err(format!("nenhum item de máquina aberto contém '{item_substr}'"));
    }
    let mut out = saida.join("\n");
    if s.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIX: &str = "\
# OVERDEV
- [ ] item livre A
- [~] implantar em produção <!-- ovf:q:abc123 -->
  - [H ] preciso que você faça o deploy <!-- ovf:q:abc123 -->
- [H ] revisar o texto da landing
- [x] item feito
";

    /// O caso que motivou tudo: responder libera o item de máquina, e o humano NÃO
    /// vira "feito" — porque ninguém fez nada, só decidiu.
    #[test]
    fn responder_libera_a_maquina_sem_mentir_que_foi_feito() {
        let r = resolver_str(FIX, &Alvo::Indice(1), Acao::Responder, "pode dar o deploy você mesmo").unwrap();
        assert!(r.texto.contains("- [H r] preciso que você faça o deploy"), "{}", r.texto);
        assert!(!r.texto.contains("- [H x]"), "responder não é fazer");
        assert!(
            r.texto.contains("- [ ] implantar em produção"),
            "o item de máquina tinha de sair do on-hold:\n{}",
            r.texto
        );
        assert!(r.texto.contains("- resposta: pode dar o deploy você mesmo"));
        assert_eq!(r.vinculado.as_deref(), Some("implantar em produção"));
    }

    /// Recusar CANCELA o item de máquina em vez de liberar — senão o agente retomaria
    /// exatamente a tarefa que a pessoa acabou de rejeitar.
    #[test]
    fn recusar_cancela_a_maquina_em_vez_de_liberar() {
        let r = resolver_str(FIX, &Alvo::Indice(1), Acao::Recusar, "não temos produção ainda").unwrap();
        assert!(r.texto.contains("- [H -] preciso que você faça o deploy"));
        assert!(r.texto.contains("- [-] implantar em produção"), "cancelado:\n{}", r.texto);
        assert!(!r.texto.contains("- [ ] implantar"), "não podia ter liberado");
        assert!(r.texto.contains("- recusado: não temos produção ainda"));
    }

    /// Item humano SEM vínculo se resolve sozinho, sem mexer em item de máquina nenhum.
    #[test]
    fn item_humano_solto_nao_afeta_maquina() {
        let r = resolver_str(FIX, &Alvo::Indice(2), Acao::Recusar, "a landing vai mudar de qualquer jeito").unwrap();
        assert!(r.texto.contains("- [H -] revisar o texto da landing"));
        assert_eq!(r.vinculado, None);
        assert!(r.texto.contains("- [~] implantar"), "o item travado ficou como estava");
    }

    /// O índice conta só entre os humanos ABERTOS — que é o que a UI numera. Contar
    /// posição absoluta fecharia o item errado.
    #[test]
    fn indice_conta_so_os_humanos_abertos() {
        let r1 = resolver_str(FIX, &Alvo::Indice(1), Acao::Responder, "x").unwrap();
        assert!(r1.item.contains("deploy"));
        let r2 = resolver_str(FIX, &Alvo::Indice(2), Acao::Responder, "x").unwrap();
        assert!(r2.item.contains("landing"));
        assert!(resolver_str(FIX, &Alvo::Indice(3), Acao::Responder, "x").is_err(), "fora de faixa");
    }

    /// Casar por texto também funciona, e só pega item ABERTO.
    #[test]
    fn casa_por_texto_e_ignora_ja_resolvidos() {
        let r = resolver_str(FIX, &Alvo::Texto("landing".into()), Acao::Responder, "ok").unwrap();
        assert!(r.texto.contains("- [H r] revisar o texto da landing"));
        // Resolvido uma vez, não casa de novo.
        assert!(resolver_str(&r.texto, &Alvo::Texto("landing".into()), Acao::Responder, "ok").is_err());
    }

    /// Resposta vazia é recusada: responder É registrar a decisão; sem texto não há
    /// decisão registrada, e o item de máquina seria liberado sem nenhum critério.
    #[test]
    fn exige_texto_nos_dois_casos() {
        assert!(resolver_str(FIX, &Alvo::Indice(1), Acao::Responder, "  ").is_err());
        assert!(resolver_str(FIX, &Alvo::Indice(1), Acao::Recusar, "").is_err());
    }

    /// O park monta o par: item vira on-hold, a pergunta entra INDENTADA logo abaixo,
    /// e os dois passam a carregar o mesmo id.
    #[test]
    fn park_cria_o_par_vinculado_e_indentado() {
        let s = "- [ ] subir o banco\n- [ ] outro item\n";
        let out = parkear_str(s, "banco", "qual região da AWS?", "zz9").unwrap();
        let linhas: Vec<&str> = out.lines().collect();
        assert!(linhas[0].starts_with("- [~] subir o banco"));
        assert!(linhas[0].contains("ovf:q:zz9"));
        assert!(linhas[1].starts_with("  - [H ] qual região da AWS?"), "indentado: {:?}", linhas[1]);
        assert!(linhas[1].contains("ovf:q:zz9"));
        assert!(linhas[2].starts_with("- [ ] outro item"), "não mexeu no resto");

        // E o par recém-criado responde/libera corretamente.
        let r = resolver_str(&out, &Alvo::Indice(1), Acao::Responder, "us-east-1").unwrap();
        assert!(r.texto.contains("- [ ] subir o banco"));
        assert_eq!(r.vinculado.as_deref(), Some("subir o banco"));
    }

    /// Item inexistente não altera nada.
    #[test]
    fn park_sem_casar_e_erro() {
        assert!(parkear_str("- [ ] a\n", "inexistente", "p", "id").is_err());
    }
}
