//! `schematize debug` — ferramenta auxiliar de diagnóstico do atualizador/versionamento.
//! O quê: dump de versão (instalada vs fonte-raw vs API), rate-limit da API do GitHub (a
//! causa clássica do "para de versionar"), caminho/gravabilidade do exe, shadows no PATH,
//! catálogo por skill (disco vs raw) e o tail do update.log. Onde: `schematize debug`.

use crate::util::{self, commands_dir, config_path, settings_path, skills_dir};
use crate::{registry, skills};
use std::path::Path;

fn hdr(t: &str) {
    println!("\n\x1b[1;36m== {t} ==\x1b[0m");
}
fn kv(k: &str, v: &str) {
    println!("  {k:<24} {v}");
}

/// Testa gravabilidade de um diretório escrevendo/apagando um arquivo-sonda.
fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".schematize-wtest");
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Rate limit da API core do GitHub: (remaining, limit, reset_epoch). None se falhar.
fn api_rate() -> Option<(i64, i64, i64)> {
    let body = util::run(
        "curl",
        &["-sfL", "-m", "8", "-H", "User-Agent: schematize-cli", "https://api.github.com/rate_limit"],
    )
    .ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let c = v.get("resources")?.get("core")?;
    Some((c.get("remaining")?.as_i64()?, c.get("limit")?.as_i64()?, c.get("reset")?.as_i64()?))
}

/// Roda o diagnóstico completo e imprime na tela.
pub fn run() {
    let cur = env!("CARGO_PKG_VERSION");

    hdr("versão do schematize (CLI)");
    kv("instalada (este bin)", cur);
    let src = skills::latest_version_raw("schematize-cli");
    kv("fonte (raw main)", src.as_deref().unwrap_or("? (raw indisponível)"));
    match &src {
        Some(s) if s == cur => kv("veredito", "atualizado ✓"),
        Some(s) => kv("veredito", &format!("DESATUALIZADO — fonte tem v{s}  →  rode: schematize upgrade")),
        None => kv("veredito", "não deu pra comparar (raw indisponível — rede?)"),
    }
    kv("API releases/latest", skills::latest_release_tag("schematize-cli").as_deref().unwrap_or("? (rate limit? ver abaixo)"));

    hdr("rate limit da API do GitHub (causa clássica do 'para de versionar')");
    match api_rate() {
        Some((rem, lim, reset)) => {
            kv("core remaining", &format!("{rem}/{lim}"));
            let now = util::now_unix() as i64;
            kv("reseta em", &format!("~{} min", ((reset - now).max(0)) / 60));
            if rem == 0 {
                println!("  \x1b[1;33m! quota ZERADA. A detecção via API falha até resetar — mas o schematize já usa raw (não a API), então isto não te trava mais.\x1b[0m");
            }
        }
        None => kv("status", "não consegui ler (rede?)"),
    }

    hdr("executável / método de instalação");
    match std::env::current_exe() {
        Ok(p) => {
            kv("current_exe", &p.display().to_string());
            if let Some(d) = p.parent() {
                kv("dir gravável?", if dir_writable(d) { "sim (self-update troca no lugar)" } else { "não (usa pkexec ou ~/.local/bin)" });
            }
        }
        Err(_) => kv("current_exe", "?"),
    }
    let shadows = util::run(
        "bash",
        &["-lc", "echo 'schematize:'; command -v -a schematize 2>/dev/null; echo 'schematize-gui:'; command -v -a schematize-gui 2>/dev/null"],
    )
    .unwrap_or_default();
    println!("  no PATH (>1 do mesmo = shadow, confunde a versão):");
    for l in shadows.lines() {
        println!("    {l}");
    }

    hdr("ambiente ~/.claude");
    kv("skills dir", &skills_dir().display().to_string());
    kv("commands dir", &commands_dir().display().to_string());
    kv("settings.json", if settings_path().exists() { "presente" } else { "ausente" });
    kv("config.json", if config_path().exists() { "presente" } else { "ausente" });

    hdr("catálogo — versão no disco vs no fonte (raw, sem API)");
    let cat = registry::catalog();
    kv("skills no catálogo", &cat.len().to_string());
    for it in &cat {
        let inst = skills::installed_version(it).unwrap_or_else(|| "—".into());
        let latest = skills::latest_version_raw(&it.repo).unwrap_or_else(|| "?".into());
        let mark = if inst == "—" {
            "não instalada"
        } else if inst == latest {
            "ok"
        } else {
            "ATUALIZAR"
        };
        println!("  {:<12} disco={:<9} raw={:<9} {}", it.slug, inst, latest, mark);
    }

    hdr("update.log (últimas tentativas de self-update)");
    let logp = config_path().parent().map(|d| d.join("update.log"));
    match logp.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(s) => {
            let lines: Vec<&str> = s.lines().collect();
            for l in lines.iter().rev().take(8).rev() {
                println!("  {l}");
            }
        }
        None => println!("  (sem update.log)"),
    }

    println!("\n\x1b[1mdica:\x1b[0m se 'fonte' > 'instalada', rode  \x1b[1mschematize upgrade\x1b[0m  (recompila do fonte, sem CI/binário).");
}
