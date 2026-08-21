//! CAIXA DE ENTRADA do overdev — injetar demandas com outro agente trabalhando.
//!
//! O quê: um lugar pra jogar "isso também precisa ser feito" enquanto o overdev roda,
//! sem interromper ninguém e sem risco de a demanda se perder. Onde: `overflow overdev
//! add` no CLI e o campo de texto da tela Overdev na GUI.
//!
//! ## Por que não escrever direto no CHECKLIST.md
//!
//! Porque haveria dois ou mais escritores no MESMO arquivo, e o ciclo é ler-modificar-
//! escrever. O agente lê o checklist, pensa por um minuto, escreve de volta; se no meio
//! disso alguém acrescentar uma linha, ela é sobrescrita — sem erro, sem aviso. O item
//! simplesmente não existe mais.
//!
//! ## O desenho, em três tempos
//!
//! 1. **Capturar** ([`adicionar`]): grava um arquivo NOVO, de nome único, em
//!    `caixa/pendente/`. Dois processos nunca disputam o mesmo caminho, então a captura
//!    não precisa de trava nenhuma e não pode falhar por concorrência. É a etapa que
//!    tem de ser infalível: é o que segura a demanda do usuário.
//! 2. **Organizar** (fora daqui): um agente lê os pendentes e devolve itens de
//!    checklist. É lento — minutos — e por isso acontece FORA de qualquer trava.
//!    Enquanto ele pensa, o overdev segue trabalhando normalmente.
//! 3. **Fundir** ([`mesclar`]): sob trava, acrescenta os itens ao checklist com escrita
//!    atômica. É a única etapa serializada, e dura milissegundos.
//!
//! ## Por que sobrevive a queda
//!
//! Cada entrada só sai de `pendente/` depois de estar em `processado/`, e só sai de
//! `processado/` depois de estar NO checklist. Toda transição é rename atômico. Queda
//! em qualquer ponto deixa a entrada no estágio anterior, e a próxima rodada refaz —
//! entrega ao-menos-uma-vez. A repetição é neutralizada por um marcador com o id da
//! entrada gravado na linha do checklist: se ele já está lá, a fusão só avança o
//! estágio em vez de duplicar o item.

use super::trava::{com_trava, escreve_atomico};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Uma demanda capturada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entrada {
    /// Id estável — nome do arquivo e marcador de idempotência no checklist.
    pub id: String,
    /// Epoch de quando foi capturada.
    pub ts: u64,
    /// O que o usuário escreveu, cru. NUNCA é reescrito — é a prova do que foi pedido.
    pub texto: String,
    /// Itens de checklist que o agente extraiu. Vazio enquanto não foi organizada.
    #[serde(default)]
    pub itens: Vec<String>,
}

/// `<overdev>/caixa`
pub fn dir_caixa(root: &Path) -> PathBuf {
    crate::paths::overdev_dir_at(root).join("caixa")
}
fn dir_pendente(root: &Path) -> PathBuf {
    dir_caixa(root).join("pendente")
}
fn dir_processado(root: &Path) -> PathBuf {
    dir_caixa(root).join("processado")
}
fn dir_feito(root: &Path) -> PathBuf {
    dir_caixa(root).join("feito")
}

/// Id único sem depender de crate de aleatoriedade: epoch em nanos + pid.
///
/// Colidir exigiria dois processos capturando no MESMO nanossegundo com o MESMO pid —
/// impossível. E o id é ordenável por tempo, o que faz a caixa processar em ordem.
/// Id curto e ordenável, reusado por quem precisa vincular duas linhas do checklist
/// (ver `overdev::resposta`). Mesma forma do id de demanda: base36 do relógio + pid.
pub fn id_curto() -> String {
    novo_id()
}

fn novo_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Base36 com largura FIXA: compacto (13 chars em vez de 39) e ainda ordenável
    // por texto, que é o que faz a caixa processar na ordem em que foi escrita.
    // Compacto importa porque o id vira um comentário em TODA linha injetada no
    // CHECKLIST.md — e esse arquivo é lido cru o tempo inteiro.
    format!("{}-{:x}", base36(nanos, 13), std::process::id())
}

/// `n` em base36, alinhado à direita com zeros até `largura`. PURO.
fn base36(mut n: u128, largura: usize) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut v = Vec::new();
    while n > 0 {
        v.push(D[(n % 36) as usize]);
        n /= 36;
    }
    while v.len() < largura {
        v.push(b'0');
    }
    v.reverse();
    String::from_utf8(v).unwrap_or_default()
}

fn agora() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// O marcador de idempotência que acompanha o item no checklist.
///
/// Comentário HTML: invisível na renderização do Markdown, mas presente no arquivo —
/// é o que permite responder "este item já entrou?" sem manter um índice à parte que
/// poderia dessincronizar do checklist.
pub fn marcador(id: &str) -> String {
    format!("<!-- ovf:{id} -->")
}

/// CAPTURA. Grava a demanda e devolve o id. Não pega trava e não lê nada compartilhado.
///
/// É a etapa que precisa ser infalível: se ela funcionar, a demanda está salva mesmo
/// que tudo depois falhe. Por isso escreve um arquivo NOVO — dois processos nunca
/// escrevem no mesmo caminho — com temporário + rename.
pub fn adicionar(root: &Path, texto: &str) -> Result<String, String> {
    let texto = texto.trim();
    if texto.is_empty() {
        return Err("demanda vazia".into());
    }
    let e = Entrada { id: novo_id(), ts: agora(), texto: texto.to_string(), itens: Vec::new() };
    let alvo = dir_pendente(root).join(format!("{}.json", e.id));
    escreve_atomico(&alvo, &serde_json::to_string_pretty(&e).map_err(|x| x.to_string())?)?;
    Ok(e.id)
}

/// Lê as entradas de um estágio, em ordem de captura.
fn ler_dir(d: &Path) -> Vec<Entrada> {
    let Ok(rd) = std::fs::read_dir(d) else { return Vec::new() };
    let mut v: Vec<(String, Entrada)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| {
            let s = std::fs::read_to_string(&p).ok()?;
            let e: Entrada = serde_json::from_str(&s).ok()?;
            Some((p.file_name()?.to_string_lossy().into_owned(), e))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.into_iter().map(|(_, e)| e).collect()
}

/// Demandas capturadas e ainda não organizadas.
pub fn pendentes(root: &Path) -> Vec<Entrada> {
    ler_dir(&dir_pendente(root))
}

/// Demandas organizadas e ainda não fundidas no checklist.
pub fn processadas(root: &Path) -> Vec<Entrada> {
    ler_dir(&dir_processado(root))
}

/// Registra os itens que o agente extraiu e avança a entrada pra `processado/`.
///
/// Grava o destino ANTES de apagar a origem: uma queda entre os dois deixa a entrada
/// nos dois lugares, e a próxima rodada só refaz um passo idempotente. A ordem
/// inversa perderia a demanda.
pub fn organizar(root: &Path, id: &str, itens: Vec<String>) -> Result<(), String> {
    let itens: Vec<String> = itens.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if itens.is_empty() {
        return Err("nenhum item — nada a registrar".into());
    }
    let origem = dir_pendente(root).join(format!("{id}.json"));
    let s = std::fs::read_to_string(&origem).map_err(|e| format!("{}: {e}", origem.display()))?;
    let mut e: Entrada = serde_json::from_str(&s).map_err(|x| x.to_string())?;
    e.itens = itens;
    let destino = dir_processado(root).join(format!("{id}.json"));
    escreve_atomico(&destino, &serde_json::to_string_pretty(&e).map_err(|x| x.to_string())?)?;
    let _ = std::fs::remove_file(&origem);
    Ok(())
}

/// FUSÃO. Acrescenta ao checklist os itens de todas as entradas processadas.
///
/// Esta é a única etapa serializada, e é curta de propósito: ler, concatenar, gravar.
/// O trabalho lento (o agente organizando) já aconteceu fora da trava.
///
/// Devolve quantos itens entraram.
pub fn mesclar(root: &Path) -> Result<usize, String> {
    let prontas = processadas(root);
    if prontas.is_empty() {
        return Ok(0);
    }
    let alvo = crate::paths::overdev_dir_at(root).join("CHECKLIST.md");
    let entrou = com_trava(&alvo, || {
        let atual = std::fs::read_to_string(&alvo).unwrap_or_default();
        let mut saida = atual.clone();
        let mut n = 0usize;
        for e in &prontas {
            // Idempotência: se o marcador já está no arquivo, esta entrada já foi
            // fundida numa rodada que caiu antes de arquivar. Não duplica.
            if atual.contains(&marcador(&e.id)) {
                continue;
            }
            if !saida.is_empty() && !saida.ends_with('\n') {
                saida.push('\n');
            }
            for item in &e.itens {
                saida.push_str(&format!("- [ ] {item} {}\n", marcador(&e.id)));
                n += 1;
            }
        }
        if n > 0 {
            escreve_atomico(&alvo, &saida)?;
        }
        Ok(n)
    })?;
    // Arquiva DEPOIS de o checklist estar no disco. Queda aqui = repetição inofensiva.
    for e in &prontas {
        let de = dir_processado(root).join(format!("{}.json", e.id));
        let para = dir_feito(root).join(format!("{}.json", e.id));
        if let Some(d) = para.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::rename(&de, &para);
    }
    Ok(entrou)
}

/// O prompt do agente que organiza as demandas.
///
/// Vive na lib porque CLI e GUI disparam o MESMO agente. E a REGRA DURA do meio é o
/// que impede o desenho de se desfazer: se o organizador editasse o checklist, ele
/// seria mais um escritor concorrente — exatamente o que a caixa existe pra evitar.
pub fn prompt_agente(bin: &str, quantas: usize) -> String {
    format!(
        "Você vai organizar demandas novas de um projeto que JÁ TEM um overdev rodando.\n\
         \n\
         REGRA DURA: não edite `CHECKLIST.md` nem nenhum arquivo do overdev. Outro agente \
         pode estar escrevendo neles agora, e sua edição sobrescreveria o trabalho dele.\n\
         \n\
         Para cada demanda listada por `{bin} overdev caixa list`:\n\
         1. leia o texto cru;\n\
         2. quebre em itens de checklist pequenos, verificáveis e independentes;\n\
         3. registre com: {bin} overdev caixa organizar <id> --item \"...\" --item \"...\"\n\
         \n\
         Quando terminar todas, rode: {bin} overdev caixa merge\n\
         Esse comando é o ÚNICO que toca o checklist, e ele o faz sob trava.\n\
         \n\
         Demandas a organizar: {quantas}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projeto(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ovf-caixa-{}-{}", std::process::id(), nome));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(crate::paths::overdev_dir_at(&d)).unwrap();
        d
    }

    /// O ciclo inteiro: captura -> organiza -> funde, e o item aparece no checklist.
    #[test]
    fn ciclo_completo_poe_o_item_no_checklist() {
        let p = projeto("ciclo");
        let cl = crate::paths::overdev_dir_at(&p).join("CHECKLIST.md");
        std::fs::write(&cl, "# OVERDEV\n- [ ] item que já existia\n").unwrap();

        let id = adicionar(&p, "precisa exportar em CSV também").unwrap();
        assert_eq!(pendentes(&p).len(), 1);
        organizar(&p, &id, vec!["adicionar exportação CSV".into(), "testar CSV com acento".into()]).unwrap();
        assert!(pendentes(&p).is_empty(), "saiu de pendente");
        assert_eq!(processadas(&p).len(), 1);

        assert_eq!(mesclar(&p).unwrap(), 2);
        let texto = std::fs::read_to_string(&cl).unwrap();
        assert!(texto.contains("item que já existia"), "não pode ter comido o que havia");
        assert!(texto.contains("- [ ] adicionar exportação CSV"));
        assert!(texto.contains("- [ ] testar CSV com acento"));
        assert!(processadas(&p).is_empty(), "arquivada em feito/");
        let _ = std::fs::remove_dir_all(&p);
    }

    /// A demanda CRUA é preservada — é a prova do que o usuário pediu, e não pode ser
    /// substituída pela interpretação que o agente fez dela.
    #[test]
    fn guarda_o_texto_cru_do_usuario() {
        let p = projeto("cru");
        let id = adicionar(&p, "  quero relatório mensal, e que ele seja em PDF  ").unwrap();
        organizar(&p, &id, vec!["gerar PDF".into()]).unwrap();
        let e = &processadas(&p)[0];
        assert_eq!(e.texto, "quero relatório mensal, e que ele seja em PDF");
        assert_eq!(e.itens, vec!["gerar PDF"]);
        let _ = std::fs::remove_dir_all(&p);
    }

    /// Fundir duas vezes NÃO duplica: é o que torna a repetição após queda inofensiva.
    #[test]
    fn fusao_repetida_nao_duplica() {
        let p = projeto("idem");
        let cl = crate::paths::overdev_dir_at(&p).join("CHECKLIST.md");
        std::fs::write(&cl, "# OVERDEV\n").unwrap();
        let id = adicionar(&p, "x").unwrap();
        organizar(&p, &id, vec!["fazer x".into()]).unwrap();
        assert_eq!(mesclar(&p).unwrap(), 1);

        // Simula a queda ENTRE gravar o checklist e arquivar: devolve a entrada
        // pro estágio anterior e funde de novo.
        std::fs::rename(
            dir_feito(&p).join(format!("{id}.json")),
            dir_processado(&p).join(format!("{id}.json")),
        )
        .unwrap();
        assert_eq!(mesclar(&p).unwrap(), 0, "reconheceu pelo marcador e não duplicou");
        let texto = std::fs::read_to_string(&cl).unwrap();
        assert_eq!(texto.matches("fazer x").count(), 1, "uma vez só:\n{texto}");
        let _ = std::fs::remove_dir_all(&p);
    }

    /// Captura concorrente: N processos jogando demanda ao mesmo tempo, nenhuma perdida.
    /// É a garantia principal — a captura não pode falhar por concorrência.
    #[test]
    fn captura_concorrente_nao_perde_demanda() {
        let p = projeto("concorrente");
        std::thread::scope(|s| {
            for i in 0..20 {
                let p = p.clone();
                s.spawn(move || {
                    adicionar(&p, &format!("demanda {i}")).unwrap();
                });
            }
        });
        assert_eq!(pendentes(&p).len(), 20, "toda demanda capturada sobreviveu");
        let _ = std::fs::remove_dir_all(&p);
    }

    /// Ids nascem ordenáveis por texto (a caixa processa na ordem em que foi escrita)
    /// e curtos (viram comentário em cada linha do checklist).
    #[test]
    fn id_e_curto_e_ordenavel() {
        assert_eq!(base36(0, 13).len(), 13);
        assert_eq!(base36(35, 4), "000z");
        assert!(base36(100, 13) < base36(101, 13), "ordem numérica = ordem textual");
        assert_eq!(base36(u64::MAX as u128, 13).len(), 13, "cabe em 13 por séculos");
        let a = novo_id();
        assert!(a.len() < 25, "curto o bastante pro artefato: {a}");
    }

    /// Demanda vazia é recusada na porta — caixa cheia de entrada em branco é ruído.
    #[test]
    fn recusa_demanda_vazia() {
        let p = projeto("vazia");
        assert!(adicionar(&p, "   ").is_err());
        assert!(pendentes(&p).is_empty());
        let _ = std::fs::remove_dir_all(&p);
    }
}
