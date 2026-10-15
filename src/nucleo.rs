//! Núcleo SEMPRE-ON — as regras de orquestração que não podem depender de a skill disparar.
//!
//! **O quê:** instala o `CORE.md` da skill `schematize-engineering` (≥ 0.29.0) em DOIS lugares
//! que o Claude Code carrega em toda sessão: (1) um bloco delimitado no `~/.claude/CLAUDE.md`
//! global; (2) um hook `SessionStart` no `~/.claude/settings.json` que imprime o `CORE.md` no
//! stdout (o Claude Code anexa o stdout do SessionStart ao contexto).
//!
//! **Por quê:** skill só carrega sob demanda. Numa máquina recém-formatada o agente ignorava a
//! regra de orquestração com subagents porque nada a colocava no contexto antes de a skill ser
//! invocada. CLAUDE.md global + hook cobrem startup, resume, clear e compact.
//!
//! **Onde:** `schematize core install|status|print`; o `doctor` (`--fix`); o `applink`, ao fim de
//! `install`/`update` delegados ao `schematize-skills`.
//!
//! ## Invariantes
//!
//! - O arquivo do usuário é dele: só o trecho entre `<!-- schematize:core:begin -->` e
//!   `<!-- schematize:core:end -->` é nosso. O resto nunca muda, byte a byte.
//! - Idempotente: rodar de novo sem mudança não escreve nada (nem toca no mtime).
//! - Escrita atômica (temp + rename) e `CLAUDE.md.bak` antes da PRIMEIRA alteração.
//! - Ler-modificar-escrever só sobre o que foi lido por inteiro: arquivo ilegível ou JSON
//!   inválido vira ERRO, nunca "arquivo vazio" sobrescrito.
//! - Sob `sudo`, age no HOME do usuário real (e devolve a posse dos arquivos a ele).

use crate::util::{escreve_atomico, ler_para_modificar};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Marca de abertura do bloco no CLAUDE.md.
pub const BEGIN: &str = "<!-- schematize:core:begin -->";
/// Marca de fechamento do bloco no CLAUDE.md.
pub const END: &str = "<!-- schematize:core:end -->";
/// Slug da skill que traz o núcleo.
pub const SKILL: &str = "schematize-engineering";
/// Trecho que identifica o nosso grupo de hook no `settings.json` (o subcomando que ele roda).
const MARCA_HOOK: &str = "core print";

/// Estado do bloco no CLAUDE.md global.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bloco {
    /// Não há bloco (ou nem há CLAUDE.md).
    Ausente,
    /// Há bloco, e o conteúdo é o do `CORE.md` instalado.
    EmDia,
    /// Há bloco com conteúdo diferente do `CORE.md` instalado. Traz a versão do marcador.
    Desatualizado { do_bloco: Option<String>, do_core: Option<String> },
}

/// Foto do núcleo numa máquina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Estado {
    /// Existe `CORE.md` na skill instalada? (Sem ele não há o que aplicar.)
    pub core_instalado: bool,
    pub versao_core: Option<String>,
    pub bloco: Bloco,
    /// O hook SessionStart está registrado (com o comando atual)?
    pub hook: bool,
}

impl Estado {
    /// Tudo aplicado e atual?
    pub fn em_dia(&self) -> bool {
        self.core_instalado && self.bloco == Bloco::EmDia && self.hook
    }
}

/// O que `instalar` fez, para o relato ao usuário.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resultado {
    pub bloco_alterado: bool,
    pub hook_alterado: bool,
    pub backup_criado: bool,
}

// ---------------------------------------------------------------------------------------------
// HOME real ("prever macacos")
// ---------------------------------------------------------------------------------------------

/// **O quê:** o HOME onde o núcleo deve ser aplicado, e a posse (uid, gid) a restaurar.
///
/// **Onde:** os wrappers públicos sem argumento de home.
///
/// Rodando como root via `sudo`, `$HOME` pode ser `/root`, e gravar lá instalaria o núcleo para
/// ninguém. Nesse caso resolve o HOME de `SUDO_USER` em `/etc/passwd`. Fora do sudo (o normal) é
/// só [`crate::util::home`].
pub fn home_real() -> (PathBuf, Option<(u32, u32)>) {
    #[cfg(unix)]
    {
        if let Some(r) = home_do_sudo() {
            return r;
        }
    }
    (crate::util::home(), None)
}

#[cfg(unix)]
fn home_do_sudo() -> Option<(PathBuf, Option<(u32, u32)>)> {
    use std::os::unix::fs::MetadataExt;
    let user = std::env::var("SUDO_USER").ok().filter(|u| !u.is_empty() && u != "root")?;
    // Só vale se este processo é root de fato (sudo -u outro, ou env herdado, não conta).
    let euid = std::process::Command::new("id").arg("-u").output().ok()?;
    if String::from_utf8_lossy(&euid.stdout).trim() != "0" {
        return None;
    }
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    let home = home_de(&passwd, &user)?;
    let dono = std::fs::metadata(&home).ok().map(|m| (m.uid(), m.gid()));
    Some((home, dono))
}

/// **O quê:** o HOME de `user` num texto no formato do `/etc/passwd`. PURA.
pub fn home_de(passwd: &str, user: &str) -> Option<PathBuf> {
    passwd.lines().find_map(|l| {
        let c: Vec<&str> = l.split(':').collect();
        (c.len() >= 6 && c[0] == user && !c[5].is_empty()).then(|| PathBuf::from(c[5]))
    })
}

/// Devolve a posse de `p` ao usuário real, quando estamos agindo por ele sob sudo.
#[allow(unused_variables)]
fn devolver_posse(p: &Path, dono: Option<(u32, u32)>) {
    #[cfg(unix)]
    {
        if let Some((u, g)) = dono {
            let _ = std::os::unix::fs::chown(p, Some(u), Some(g));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Caminhos
// ---------------------------------------------------------------------------------------------

/// `<home>/.claude/skills/schematize-engineering/assets/CORE.md`.
pub fn core_path(home: &Path) -> PathBuf {
    home.join(".claude").join("skills").join(SKILL).join("assets").join("CORE.md")
}

fn claude_md(home: &Path) -> PathBuf {
    home.join(".claude").join("CLAUDE.md")
}

fn settings(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

// ---------------------------------------------------------------------------------------------
// Bloco do CLAUDE.md (funções PURAS)
// ---------------------------------------------------------------------------------------------

/// **O quê:** a versão do marcador `<!-- schematize-core vX.Y.Z -->` (1ª linha não vazia). PURA.
pub fn versao_do_marcador(texto: &str) -> Option<String> {
    let l = texto.lines().find(|l| !l.trim().is_empty())?.trim();
    let r = l.strip_prefix("<!--")?.trim_start().strip_prefix("schematize-core")?;
    let v = r.trim_start().strip_suffix("-->")?.trim();
    let v = v.strip_prefix('v').unwrap_or(v);
    (!v.is_empty()).then(|| v.to_string())
}

/// Localiza o bloco: `(início_do_begin, fim_depois_do_end)` em bytes. `Err` se o begin não tem
/// end (estado corrompido: não adivinhamos até onde vai o que é nosso).
fn localizar(texto: &str) -> Result<Option<(usize, usize)>, String> {
    let Some(i) = texto.find(BEGIN) else { return Ok(None) };
    let resto = &texto[i + BEGIN.len()..];
    match resto.find(END) {
        Some(j) => Ok(Some((i, i + BEGIN.len() + j + END.len()))),
        None => Err(format!(
            "CLAUDE.md tem `{BEGIN}` sem `{END}`. Não vou adivinhar até onde vai o bloco — \
             conserte à mão (apague a marca solta) e rode de novo."
        )),
    }
}

/// **O quê:** o conteúdo que está entre as marcas, sem as marcas e sem as quebras de borda. PURA.
pub fn conteudo_do_bloco(texto: &str) -> Result<Option<String>, String> {
    Ok(localizar(texto)?.map(|(i, f)| {
        texto[i + BEGIN.len()..f - END.len()].trim_matches(['\n', '\r']).trim_end().to_string()
    }))
}

/// **O quê:** `existente` com o bloco do núcleo aplicado. PURA — o coração do merge.
///
/// - com bloco: substitui SÓ o trecho `begin..end` (antes e depois ficam byte a byte);
/// - sem bloco: anexa no fim, separado por uma linha em branco;
/// - vazio: o arquivo passa a ser só o bloco.
pub fn aplicar_bloco(existente: &str, core: &str) -> Result<String, String> {
    let bloco = format!("{BEGIN}\n{}\n{END}", core.trim_matches(['\n', '\r']).trim_end());
    if let Some((i, f)) = localizar(existente)? {
        return Ok(format!("{}{bloco}{}", &existente[..i], &existente[f..]));
    }
    if existente.trim().is_empty() {
        return Ok(format!("{bloco}\n"));
    }
    let sep = if existente.ends_with("\n\n") {
        ""
    } else if existente.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok(format!("{existente}{sep}{bloco}\n"))
}

// ---------------------------------------------------------------------------------------------
// Hook SessionStart (funções PURAS sobre o JSON)
// ---------------------------------------------------------------------------------------------

/// **O quê:** o comando do hook: resolve o `schematize` em runtime e roda `core print`.
///
/// Mesmo padrão dos hooks do overdev: candidatos de binário em ordem, `exec` no primeiro que
/// existe, e **exit 0 silencioso** se nenhum existir — um hook de SessionStart que sai não-zero
/// ou imprime ruído estraga a abertura da sessão por causa de um binário ausente.
pub fn hook_cmd() -> String {
    r#"for c in "$HOME/.cargo/bin/schematize" "$HOME/.local/bin/schematize" /usr/local/bin/schematize /usr/bin/schematize "$(command -v schematize 2>/dev/null)"; do [ -n "$c" ] && [ -x "$c" ] && exec "$c" core print; done; exit 0"#.to_string()
}

fn grupo_tem_marca(g: &Value) -> bool {
    g.get("hooks").and_then(|h| h.as_array()).is_some_and(|a| {
        a.iter().any(|h| {
            h.get("command").and_then(|c| c.as_str()).is_some_and(|c| c.contains(MARCA_HOOK))
        })
    })
}

/// **O quê:** o hook SessionStart do núcleo está em `root`, com o comando ATUAL? PURA.
pub fn hook_presente(root: &Value) -> bool {
    let cmd = hook_cmd();
    root.get("hooks").and_then(|h| h.get("SessionStart")).and_then(|a| a.as_array()).is_some_and(
        |a| {
            a.iter().any(|g| {
                g.get("hooks").and_then(|h| h.as_array()).is_some_and(|hs| {
                    hs.iter().any(|h| h.get("command").and_then(|c| c.as_str()) == Some(&cmd))
                })
            })
        },
    )
}

/// **O quê:** registra o hook em `root`; devolve se mudou algo. PURA e IDEMPOTENTE.
///
/// Sem matcher (vale em startup/resume/clear/compact). Preserva os demais hooks e o resto do
/// JSON. Um grupo NOSSO com comando velho é atualizado no lugar; nunca duplica.
pub fn registrar_hook(root: &mut Value) -> bool {
    if hook_presente(root) {
        return false;
    }
    let Some(obj) = root.as_object_mut() else { return false };
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let Some(hooks) = hooks.as_object_mut() else { return false };
    let arr = hooks.entry("SessionStart").or_insert_with(|| json!([]));
    if !arr.is_array() {
        *arr = json!([]);
    }
    let Some(arr) = arr.as_array_mut() else { return false };
    let novo = json!({ "hooks": [ { "type": "command", "command": hook_cmd() } ] });
    // Atualiza o PRIMEIRO grupo nosso no lugar e descarta duplicatas antigas, se houver.
    match arr.iter().position(grupo_tem_marca) {
        Some(p) => {
            arr[p] = novo;
            let mut i = 0;
            arr.retain(|g| {
                let manter = i <= p || !grupo_tem_marca(g);
                i += 1;
                manter
            });
        }
        None => arr.push(novo),
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Operações sobre um HOME
// ---------------------------------------------------------------------------------------------

fn ler_core(home: &Path) -> Option<String> {
    std::fs::read_to_string(core_path(home)).ok().filter(|s| !s.trim().is_empty())
}

fn ler_settings(home: &Path) -> Result<Value, String> {
    let p = settings(home);
    let t = ler_para_modificar(&p)?;
    if t.trim().is_empty() {
        return Ok(json!({}));
    }
    let v: Value = serde_json::from_str(&t).map_err(|e| {
        format!("{}: JSON inválido ({e}). Não vou sobrescrever; conserte o arquivo.", p.display())
    })?;
    if v.is_object() {
        Ok(v)
    } else {
        Err(format!("{}: a raiz não é um objeto JSON; não vou sobrescrever.", p.display()))
    }
}

/// **O quê:** lê o estado do núcleo no `home` dado. Nunca escreve.
pub fn estado_em(home: &Path) -> Estado {
    let core = ler_core(home);
    let versao_core = core.as_deref().and_then(versao_do_marcador);
    let bloco = match (std::fs::read_to_string(claude_md(home)).ok(), &core) {
        (Some(t), Some(c)) => match conteudo_do_bloco(&t) {
            Ok(Some(b)) if b == c.trim_matches(['\n', '\r']).trim_end() => Bloco::EmDia,
            Ok(Some(b)) => Bloco::Desatualizado {
                do_bloco: versao_do_marcador(&b),
                do_core: versao_core.clone(),
            },
            // Bloco corrompido conta como desatualizado: o `install` explica o erro.
            Err(_) => Bloco::Desatualizado { do_bloco: None, do_core: versao_core.clone() },
            Ok(None) => Bloco::Ausente,
        },
        (Some(t), None) => match conteudo_do_bloco(&t) {
            Ok(Some(b)) => Bloco::Desatualizado { do_bloco: versao_do_marcador(&b), do_core: None },
            _ => Bloco::Ausente,
        },
        (None, _) => Bloco::Ausente,
    };
    let hook = ler_settings(home).map(|v| hook_presente(&v)).unwrap_or(false);
    Estado { core_instalado: core.is_some(), versao_core, bloco, hook }
}

/// **O quê:** aplica o bloco no CLAUDE.md e registra o hook, sob o `home` dado.
///
/// `dono` é a posse a devolver aos arquivos (sudo). Falha ANTES de escrever qualquer coisa se
/// algo não puder ser lido por inteiro, então um erro nunca deixa metade aplicada por causa de
/// entrada inválida.
pub fn instalar_em(home: &Path, dono: Option<(u32, u32)>) -> Result<Resultado, String> {
    let core = ler_core(home).ok_or_else(|| {
        format!(
            "não achei {}: instale/atualize a skill {SKILL} (≥ 0.29.0) — `schematize install engineering`.",
            core_path(home).display()
        )
    })?;
    // Lê e valida TUDO antes de gravar.
    let cm = claude_md(home);
    let atual = ler_para_modificar(&cm)?;
    let novo = aplicar_bloco(&atual, &core)?;
    let mut st = ler_settings(home)?;
    let mut r = Resultado::default();

    if novo != atual {
        if let Some(dir) = cm.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let bak = cm.with_file_name("CLAUDE.md.bak");
        if cm.exists() && !bak.exists() {
            std::fs::copy(&cm, &bak).map_err(|e| format!("backup {}: {e}", bak.display()))?;
            devolver_posse(&bak, dono);
            r.backup_criado = true;
        }
        escreve_atomico(&cm, &novo)?;
        devolver_posse(&cm, dono);
        r.bloco_alterado = true;
    }
    if registrar_hook(&mut st) {
        let corpo = serde_json::to_string_pretty(&st).map_err(|e| e.to_string())?;
        let sp = settings(home);
        if let Some(dir) = sp.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        escreve_atomico(&sp, &(corpo + "\n"))?;
        devolver_posse(&sp, dono);
        r.hook_alterado = true;
    }
    Ok(r)
}

// ---------------------------------------------------------------------------------------------
// API de alto nível (HOME real) — o que o CLI, o doctor e o applink chamam
// ---------------------------------------------------------------------------------------------

/// Estado no HOME real.
pub fn estado() -> Estado {
    estado_em(&home_real().0)
}

/// Instala no HOME real.
pub fn instalar() -> Result<Resultado, String> {
    let (h, dono) = home_real();
    instalar_em(&h, dono)
}

/// O conteúdo do `CORE.md` instalado (HOME real), para o hook. `None` = ausente.
pub fn texto_core() -> Option<String> {
    ler_core(&home_real().0)
}

/// **O quê:** a engineering está instalada (há pasta da skill)? HOME real.
pub fn engineering_instalada() -> bool {
    home_real().0.join(".claude").join("skills").join(SKILL).is_dir()
}

/// **O quê:** garante o núcleo SE a engineering está instalada; relata numa linha. Best-effort.
///
/// **Onde:** fim de `install`/`update` (applink). Nunca falha o comando que o chamou: o núcleo é
/// reforço, e o `doctor --fix` corrige depois.
pub fn garantir_apos_install() {
    if !engineering_instalada() {
        return;
    }
    match instalar() {
        Ok(r) if r.bloco_alterado || r.hook_alterado => {
            println!("núcleo sempre-on aplicado (CLAUDE.md global + hook SessionStart).")
        }
        Ok(_) => {}
        Err(e) => eprintln!("aviso: não apliquei o núcleo sempre-on: {e}"),
    }
}

/// Texto humano do estado (para `core status` e `doctor`).
pub fn descrever(s: &Estado) -> String {
    let bloco = match &s.bloco {
        Bloco::Ausente => "bloco no CLAUDE.md: ausente".to_string(),
        Bloco::EmDia => "bloco no CLAUDE.md: em dia".to_string(),
        Bloco::Desatualizado { do_bloco, do_core } => format!(
            "bloco no CLAUDE.md: desatualizado ({} → {})",
            do_bloco.as_deref().unwrap_or("?"),
            do_core.as_deref().unwrap_or("?")
        ),
    };
    let core = match (&s.core_instalado, &s.versao_core) {
        (true, Some(v)) => format!("CORE.md instalado: v{v}"),
        (true, None) => "CORE.md instalado: versão sem marcador".to_string(),
        _ => "CORE.md instalado: ausente (skill schematize-engineering ≥ 0.29.0)".to_string(),
    };
    let hook = if s.hook { "hook SessionStart: presente" } else { "hook SessionStart: ausente" };
    format!("{core}\n{bloco}\n{hook}")
}

#[cfg(test)]
mod tests;
