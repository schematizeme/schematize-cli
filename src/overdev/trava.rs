//! TRAVA entre processos + escrita atômica do checklist.
//!
//! O problema, que é real e já existia antes desta feature: todo escritor do
//! `CHECKLIST.md` fazia `ler → modificar → fs::write`. Sem serialização, dois
//! escritores simultâneos (dois agentes, ou um agente e a GUI) fazem o segundo
//! sobrescrever o trabalho do primeiro — e o item some sem erro nenhum. Pior: um
//! `fs::write` interrompido no meio deixa o arquivo TRUNCADO, e o checklist é o
//! estado do projeto.
//!
//! Duas garantias, e elas são independentes:
//!
//!  1. **Atomicidade** ([`escreve_atomico`]): grava num temporário do MESMO diretório
//!     e renomeia por cima. `rename(2)` é atômico — ou o arquivo é o antigo inteiro,
//!     ou o novo inteiro, nunca metade. Sem isto, um desligamento no momento errado
//!     custa o checklist.
//!  2. **Exclusão mútua** ([`com_trava`]): só um processo por vez faz o ciclo
//!     ler-modificar-escrever. `create_new` é a criação atômica que o SO garante —
//!     quem cria, ganha.
//!
//! O que este módulo NÃO faz: segurar a trava durante trabalho lento. Um agente
//! organizando demandas leva minutos; travar o checklist por minutos é o mesmo que
//! travar o projeto. A trava cobre só a fusão, que é de milissegundos — o trabalho
//! lento acontece fora dela (ver [`super::caixa`]).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Quanto tempo insistir antes de desistir de pegar a trava.
const ESPERA_MAX: Duration = Duration::from_secs(10);
/// A partir de quanto tempo uma trava é candidata a ÓRFÃ (dona morreu sem soltar).
const ORFA_APOS: Duration = Duration::from_secs(60);

/// Trava presa. Solta no `Drop` — inclusive se o corpo entrar em pânico, que é
/// justamente quando esquecer de soltar deixaria o projeto travado pra sempre.
pub struct Guarda {
    arquivo: PathBuf,
}

impl Drop for Guarda {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.arquivo);
    }
}

/// Caminho do arquivo de trava que protege `alvo`.
fn arquivo_de_trava(alvo: &Path) -> PathBuf {
    let nome = alvo.file_name().and_then(|s| s.to_str()).unwrap_or("alvo");
    alvo.with_file_name(format!(".{nome}.trava"))
}

/// O processo `pid` ainda está vivo?
///
/// Só isso separa "alguém está trabalhando" de "alguém morreu segurando a trava".
/// Sem esta checagem, um `kill -9` no momento errado travaria o projeto até alguém
/// apagar o arquivo à mão — e ninguém sabe que precisa.
fn vivo(pid: u32) -> bool {
    if cfg!(target_os = "linux") {
        return Path::new("/proc").join(pid.to_string()).exists();
    }
    // Sem /proc não dá pra saber sem libc; assume vivo e deixa a idade decidir.
    true
}

/// A trava em `arquivo` está órfã: velha o bastante E sem dono vivo.
///
/// O limiar de idade é PARÂMETRO, não constante embutida. Não é generalidade
/// gratuita: o std não expõe `set_mtime`, então com a constante embutida o teste
/// teria de envelhecer o arquivo de mentira — um teste que não testa nada. Com o
/// limiar injetado, o teste exercita a decisão de verdade e não depende do relógio.
/// A produção chama sempre com [`ORFA_APOS`].
///
/// Existe pra o teste poder exercitar a decisão de verdade: o std não expõe
/// `set_mtime`, então a alternativa seria um helper que finge envelhecer o arquivo —
/// um teste que não testa nada. Injetar o limiar é honesto e não depende do relógio.
fn orfa_apos(arquivo: &Path, limiar: Duration) -> bool {
    let Ok(md) = std::fs::metadata(arquivo) else { return false };
    let idade = md.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok());
    if idade.is_none_or(|d| d < limiar) {
        return false;
    }
    // Velha. Só é órfã se o dono não estiver mais vivo.
    match std::fs::read_to_string(arquivo).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
        Some(pid) => !vivo(pid),
        None => true, // sem pid legível: velha e anônima, pode ir
    }
}

/// Tenta pegar a trava UMA vez. `None` = já está com outro.
fn tenta(arquivo: &Path) -> Option<Guarda> {
    if let Some(d) = arquivo.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // `create_new` = O_CREAT|O_EXCL: a criação é atômica no SO. Quem cria, ganha —
    // não há janela entre "checar se existe" e "criar" pra dois processos passarem.
    match std::fs::OpenOptions::new().write(true).create_new(true).open(arquivo) {
        Ok(mut f) => {
            let _ = write!(f, "{}", std::process::id());
            let _ = f.flush();
            Some(Guarda { arquivo: arquivo.to_path_buf() })
        }
        Err(_) => None,
    }
}

/// Roda `f` com exclusividade sobre `alvo`.
///
/// Espera até [`ESPERA_MAX`] por uma trava de outro processo. Trava órfã (dona morta)
/// é quebrada. Estourou a espera com um dono VIVO: devolve erro em vez de forçar —
/// atropelar quem está trabalhando é exatamente o que a trava existe pra impedir.
pub fn com_trava<T>(alvo: &Path, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    com_trava_cfg(alvo, ORFA_APOS, ESPERA_MAX, f)
}

/// [`com_trava`] com os tempos INJETADOS — ver a nota em [`orfa_apos`].
fn com_trava_cfg<T>(
    alvo: &Path,
    orfa_apos_: Duration,
    espera_max: Duration,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let arquivo = arquivo_de_trava(alvo);
    let inicio = std::time::Instant::now();
    loop {
        if let Some(_g) = tenta(&arquivo) {
            // `_g` vive até o fim do escopo: solta a trava mesmo se `f` entrar em pânico.
            return f();
        }
        if orfa_apos(&arquivo, orfa_apos_) {
            let _ = std::fs::remove_file(&arquivo);
            continue;
        }
        if inicio.elapsed() >= espera_max {
            return Err(format!(
                "outro processo está editando {} há mais de {}s — tente de novo em instantes",
                alvo.display(),
                espera_max.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Grava `conteudo` em `alvo` de forma ATÔMICA: temporário no mesmo diretório + rename.
///
/// O temporário TEM de ser no mesmo diretório: `rename` entre sistemas de arquivos
/// falha com `EXDEV`, e um `/tmp` em outro FS é o caso comum. `sync_all` antes do
/// rename pra o conteúdo estar no disco quando o nome passar a apontar pra ele.
pub fn escreve_atomico(alvo: &Path, conteudo: &str) -> Result<(), String> {
    let dir = alvo.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        alvo.file_name().and_then(|s| s.to_str()).unwrap_or("alvo"),
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(conteudo.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, alvo).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", alvo.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn scratch(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ovf-trava-{}-{}", std::process::id(), nome));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("CHECKLIST.md")
    }

    /// O ponto do módulo: dois ciclos ler-modificar-escrever concorrentes NÃO se
    /// perdem. Sem a trava este teste falha — é o bug que ele existe pra fixar.
    #[test]
    fn escritas_concorrentes_nao_se_perdem() {
        let alvo = scratch("concorrente");
        escreve_atomico(&alvo, "").unwrap();
        let alvo = Arc::new(alvo);
        let feitos = Arc::new(AtomicUsize::new(0));

        std::thread::scope(|s| {
            for i in 0..8 {
                let alvo = alvo.clone();
                let feitos = feitos.clone();
                s.spawn(move || {
                    let r = com_trava(&alvo, || {
                        // ler → modificar → escrever, com uma pausa no meio pra
                        // garantir que as janelas se sobreponham de verdade.
                        let atual = std::fs::read_to_string(&*alvo).unwrap_or_default();
                        std::thread::sleep(Duration::from_millis(5));
                        escreve_atomico(&alvo, &format!("{atual}linha {i}\n"))
                    });
                    if r.is_ok() {
                        feitos.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
        });

        assert_eq!(feitos.load(Ordering::SeqCst), 8, "todas deviam ter conseguido a trava");
        let final_ = std::fs::read_to_string(&*alvo).unwrap();
        assert_eq!(final_.lines().count(), 8, "nenhuma escrita foi perdida:\n{final_}");
        let _ = std::fs::remove_dir_all(alvo.parent().unwrap());
    }

    /// Trava velha cujo dono morreu é quebrada — senão um `kill -9` travaria o
    /// projeto pra sempre e ninguém saberia que é preciso apagar um arquivo à mão.
    #[test]
    fn trava_orfa_e_quebrada() {
        let alvo = scratch("orfa");
        let lock = arquivo_de_trava(&alvo);
        // pid que não existe; limiar ZERO faz qualquer arquivo contar como velho, então
        // quem decide é só o critério que interessa aqui: o dono está vivo?
        std::fs::write(&lock, "4294967290").unwrap();
        assert!(orfa_apos(&lock, Duration::ZERO), "dona morta = órfã");

        let ok = com_trava_cfg(&alvo, Duration::ZERO, Duration::from_secs(2), || {
            escreve_atomico(&alvo, "passou\n")
        });
        assert!(ok.is_ok(), "devia ter quebrado a trava órfã: {ok:?}");
        assert_eq!(std::fs::read_to_string(&alvo).unwrap(), "passou\n");
        let _ = std::fs::remove_dir_all(alvo.parent().unwrap());
    }

    /// Trava de dono VIVO não é quebrada nem por idade — atropelar quem está
    /// trabalhando é exatamente o que a trava impede. E o pedido FALHA com mensagem,
    /// em vez de forçar: perder o trabalho do outro seria pior que recusar o meu.
    #[test]
    fn trava_de_dono_vivo_resiste_e_o_pedido_falha() {
        let alvo = scratch("vivo");
        let lock = arquivo_de_trava(&alvo);
        std::fs::write(&lock, std::process::id().to_string()).unwrap();
        assert!(!orfa_apos(&lock, Duration::ZERO), "dono vivo: velha, mas não órfã");

        let r = com_trava_cfg(&alvo, Duration::ZERO, Duration::from_millis(200), || {
            escreve_atomico(&alvo, "NAO DEVIA\n")
        });
        assert!(r.is_err(), "não podia ter atropelado um dono vivo");
        assert!(!alvo.exists(), "e não escreveu nada");
        let _ = std::fs::remove_dir_all(alvo.parent().unwrap());
    }

    /// A escrita atômica não deixa temporário pra trás e substitui por completo.
    #[test]
    fn escrita_atomica_nao_deixa_lixo() {
        let alvo = scratch("atomica");
        escreve_atomico(&alvo, "primeiro\n").unwrap();
        escreve_atomico(&alvo, "segundo\n").unwrap();
        assert_eq!(std::fs::read_to_string(&alvo).unwrap(), "segundo\n");
        let sobrou: Vec<_> = std::fs::read_dir(alvo.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(sobrou.is_empty(), "temporários vazados: {sobrou:?}");
        let _ = std::fs::remove_dir_all(alvo.parent().unwrap());
    }

}
