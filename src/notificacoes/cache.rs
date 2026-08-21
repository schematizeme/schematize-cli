//! CACHE local das notificações — com histórico.
//!
//! ## O bug que isto conserta
//!
//! `count()` era `collect().len()`: o badge fazia a rodada de rede INTEIRA (feed do
//! blog + versões das skills + as do servidor) e, ao abrir o painel, ela era feita de
//! novo. Duas idas independentes à rede pra a mesma pergunta. Quando a segunda falhava
//! ou demorava, o badge dizia "3" e o painel abria vazio — "marca e não carrega". E um
//! timer repetia isso a cada 90 segundos.
//!
//! Aqui o cache passa a ser a ÚNICA fonte da UI: o badge conta o cache, o painel lê o
//! cache, e a rede só ALIMENTA o cache, em segundo plano. Rede fora do ar deixa de ser
//! "some tudo" e vira "não chega nada novo".
//!
//! ## Histórico
//!
//! Notificação resolvida não desaparece — muda de [`Estado`]. Some da contagem, sai do
//! topo da lista, e continua consultável. Apagar seria perder o registro de que o aviso
//! existiu, que é justamente o que se quer olhar depois ("quando essa skill ficou
//! desatualizada?").
//!
//! ## Identidade estável
//!
//! Cada notificação tem um `id` derivado do conteúdo (tipo + título + ação). É ele que
//! faz uma recoleta RECONHECER o que já estava lá em vez de duplicar, e é o que preserva
//! o estado entre atualizações. Sem id estável, "marcar como lida" se perderia no
//! próximo refresh e o badge voltaria sozinho.

use super::formato::{Acao, Kind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Em que ponto do ciclo a notificação está.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Estado {
    /// Nunca vista. É o que o badge conta.
    Nova,
    /// Já apareceu na tela pro usuário.
    Lida,
    /// A ação foi tomada (app atualizado, skill atualizada). Vai pro histórico.
    Concluida,
}

/// Uma notificação persistida.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registro {
    pub id: String,
    /// "global" | "personal".
    pub escopo: String,
    pub titulo: String,
    pub corpo: String,
    /// Já validado contra o conjunto fechado (ver `formato::Kind`).
    pub kind: String,
    /// Já validada (`https` ou comando interno de forma fechada); vazia = sem ação.
    pub acao: String,
    pub estado: Estado,
    /// Epoch de quando apareceu pela 1ª vez. Não muda em recoletas.
    pub visto_em: u64,
    /// Epoch da última mudança de estado.
    pub mexido_em: u64,
}

/// Teto do histórico. Sem ele o arquivo cresce pra sempre; com ele, o que sai é o mais
/// antigo JÁ CONCLUÍDO — nunca algo que ainda pede atenção.
pub const MAX_HISTORICO: usize = 500;

/// Id estável a partir do conteúdo. FNV-1a: determinístico, sem dependência, e aqui
/// não precisa de resistência criptográfica — precisa de estabilidade.
pub fn id_de(kind: &str, titulo: &str, acao: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in kind.bytes().chain(b"\x1f".iter().copied()).chain(titulo.bytes()).chain(b"\x1f".iter().copied()).chain(acao.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn agora() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Onde o cache mora.
pub fn arquivo() -> PathBuf {
    crate::util::dados_dir().join("notifications.json")
}

/// Lê o cache. Ausente ou corrompido = vazio — nunca impede o app de abrir.
pub fn ler() -> Vec<Registro> {
    std::fs::read_to_string(arquivo())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Grava o cache de forma ATÔMICA (temporário + rename, via `overdev::trava`).
///
/// Atômica porque este arquivo é lido pelo badge a cada abertura: um arquivo escrito
/// pela metade viraria "nenhuma notificação" — o mesmo sintoma que estamos corrigindo.
pub fn gravar(v: &[Registro]) -> Result<(), String> {
    let corpo = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    crate::overdev::trava::escreve_atomico(&arquivo(), &corpo)
}

/// Funde o que acabou de chegar da coleta com o que já estava guardado.
///
/// PURA (recebe o cache e as novas, devolve o cache novo) porque é aqui que mora a
/// regra que mais importa: **estado já existente é preservado**. Se a fusão sobrescrevesse,
/// toda recoleta ressuscitaria o que o usuário já tinha lido, e o badge voltaria sozinho
/// — que é a forma mais rápida de treinar alguém a ignorar notificação.
pub fn fundir(atual: &[Registro], novas: Vec<Registro>) -> Vec<Registro> {
    let mut out: Vec<Registro> = atual.to_vec();
    for n in novas {
        match out.iter().position(|r| r.id == n.id) {
            // Já conhecida: atualiza só o TEXTO (o servidor pode ter corrigido) e
            // mantém estado e data de primeira aparição.
            Some(i) => {
                out[i].titulo = n.titulo;
                out[i].corpo = n.corpo;
                out[i].acao = n.acao;
                out[i].escopo = n.escopo;
            }
            None => out.push(n),
        }
    }
    // Mais recente primeiro; concluídas afundam.
    out.sort_by(|a, b| {
        let peso = |r: &Registro| match r.estado {
            Estado::Nova => 0,
            Estado::Lida => 1,
            Estado::Concluida => 2,
        };
        peso(a).cmp(&peso(b)).then(b.visto_em.cmp(&a.visto_em))
    });
    // Poda: só CONCLUÍDAS saem, e da cauda. Nunca se descarta o que ainda pede atenção.
    if out.len() > MAX_HISTORICO {
        let mut mantidas: Vec<Registro> = out.iter().filter(|r| r.estado != Estado::Concluida).cloned().collect();
        let concluidas: Vec<Registro> = out.into_iter().filter(|r| r.estado == Estado::Concluida).collect();
        let cabem = MAX_HISTORICO.saturating_sub(mantidas.len());
        mantidas.extend(concluidas.into_iter().take(cabem));
        return mantidas;
    }
    out
}

/// Constrói um `Registro` novo a partir de dados JÁ SANITIZADOS.
pub fn novo(escopo: &str, kind: &Kind, titulo: String, corpo: String, acao: &Acao) -> Registro {
    let k = kind.como_str().to_string();
    let a = acao.como_str().to_string();
    let t = agora();
    Registro {
        id: id_de(&k, &titulo, &a),
        escopo: escopo.to_string(),
        titulo,
        corpo,
        kind: k,
        acao: a,
        estado: Estado::Nova,
        visto_em: t,
        mexido_em: t,
    }
}

/// Quantas ainda pedem atenção (o número do badge). Só as NOVAS.
pub fn nao_lidas(v: &[Registro]) -> usize {
    v.iter().filter(|r| r.estado == Estado::Nova).count()
}

/// Muda o estado de uma notificação. `false` se o id não existe.
pub fn marcar(v: &mut [Registro], id: &str, estado: Estado) -> bool {
    match v.iter_mut().find(|r| r.id == id) {
        Some(r) => {
            r.estado = estado;
            r.mexido_em = agora();
            true
        }
        None => false,
    }
}

/// Marca como LIDA tudo que está NOVO (o painel foi aberto).
pub fn marcar_todas_lidas(v: &mut [Registro]) -> usize {
    let t = agora();
    let mut n = 0;
    for r in v.iter_mut().filter(|r| r.estado == Estado::Nova) {
        r.estado = Estado::Lida;
        r.mexido_em = t;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(id_titulo: &str, estado: Estado, visto: u64) -> Registro {
        Registro {
            id: id_de("news", id_titulo, ""),
            escopo: "global".into(),
            titulo: id_titulo.into(),
            corpo: "x".into(),
            kind: "news".into(),
            acao: String::new(),
            estado,
            visto_em: visto,
            mexido_em: visto,
        }
    }

    /// A regra central: recoletar NÃO ressuscita o que já foi lido. Se ressuscitasse, o
    /// badge voltaria sozinho a cada 90s e o usuário aprenderia a ignorá-lo.
    #[test]
    fn recoleta_nao_ressuscita_o_que_ja_foi_lido() {
        let atual = vec![reg("post A", Estado::Lida, 100), reg("post B", Estado::Concluida, 90)];
        // A coleta devolve as MESMAS, como sempre volta (elas continuam no feed).
        let novas = vec![reg("post A", Estado::Nova, 200), reg("post B", Estado::Nova, 200)];
        let out = fundir(&atual, novas);
        assert_eq!(out.len(), 2, "não duplicou");
        assert_eq!(out.iter().find(|r| r.titulo == "post A").unwrap().estado, Estado::Lida);
        assert_eq!(out.iter().find(|r| r.titulo == "post B").unwrap().estado, Estado::Concluida);
        assert_eq!(nao_lidas(&out), 0, "o badge não volta sozinho");
    }

    /// Notificação nova de verdade entra e conta.
    #[test]
    fn nova_entra_e_conta_no_badge() {
        let atual = vec![reg("velha", Estado::Lida, 100)];
        let out = fundir(&atual, vec![reg("nova", Estado::Nova, 200)]);
        assert_eq!(out.len(), 2);
        assert_eq!(nao_lidas(&out), 1);
        assert_eq!(out[0].titulo, "nova", "não-lida vem primeiro");
    }

    /// Resolver NÃO apaga: vira histórico, some da contagem, continua consultável.
    #[test]
    fn concluir_mantem_historico() {
        let mut v = vec![reg("a", Estado::Nova, 100)];
        let id = v[0].id.clone();
        assert!(marcar(&mut v, &id, Estado::Concluida));
        assert_eq!(v.len(), 1, "continua existindo");
        assert_eq!(v[0].estado, Estado::Concluida);
        assert_eq!(nao_lidas(&v), 0);
        assert!(!marcar(&mut v, "id-que-nao-existe", Estado::Lida));
    }

    /// Abrir o painel zera o badge sem apagar nada.
    #[test]
    fn abrir_o_painel_marca_lidas_sem_perder_item() {
        let mut v = vec![reg("a", Estado::Nova, 1), reg("b", Estado::Nova, 2), reg("c", Estado::Concluida, 3)];
        assert_eq!(marcar_todas_lidas(&mut v), 2);
        assert_eq!(v.len(), 3);
        assert_eq!(nao_lidas(&v), 0);
        assert_eq!(v.iter().filter(|r| r.estado == Estado::Concluida).count(), 1, "concluída não virou lida");
    }

    /// A poda tira só CONCLUÍDAS. O que ainda pede atenção nunca é descartado por teto.
    #[test]
    fn poda_nunca_descarta_o_que_pede_atencao() {
        let atual: Vec<Registro> = (0..MAX_HISTORICO + 50)
            .map(|i| reg(&format!("c{i}"), Estado::Concluida, i as u64))
            .collect();
        let novas: Vec<Registro> = (0..10).map(|i| reg(&format!("n{i}"), Estado::Nova, 9999)).collect();
        let out = fundir(&atual, novas);
        assert!(out.len() <= MAX_HISTORICO);
        assert_eq!(out.iter().filter(|r| r.estado == Estado::Nova).count(), 10, "as 10 novas sobreviveram");
    }

    /// O id vem do CONTEÚDO: mesma notificação = mesmo id em qualquer execução. É o que
    /// segura o estado entre refreshes.
    #[test]
    fn id_e_estavel_e_discrimina() {
        assert_eq!(id_de("news", "t", "a"), id_de("news", "t", "a"));
        assert_ne!(id_de("news", "t", "a"), id_de("news", "t", "b"));
        assert_ne!(id_de("news", "t", "a"), id_de("server", "t", "a"));
        // Separador impede colisão por concatenação ("ab"+"c" vs "a"+"bc").
        assert_ne!(id_de("news", "ab", "c"), id_de("news", "a", "bc"));
    }
}
