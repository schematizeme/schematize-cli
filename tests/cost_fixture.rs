//! O QUE: prova o `cost sync` ponta a ponta sobre transcripts SINTÉTICOS (tests/fixtures/cost).
//!
//! POR QUE EXISTE: os testes unitários montam linhas em memória; este lê arquivos no formato
//! real do Claude Code, com streaming repetido, linha truncada e subagent, e confere o JSON
//! que a GUI consome. As fixtures não têm texto de conversa.

use chrono::FixedOffset;
use schematize::cost::projeto::{slug, Projeto};
use schematize::cost::sync::{relatar, sincronizar, Ambiente};
use std::path::Path;

fn copiar(de: &Path, para: &Path) {
    std::fs::create_dir_all(para).unwrap();
    for e in std::fs::read_dir(de).unwrap().flatten() {
        let (p, d) = (e.path(), para.join(e.file_name()));
        if p.is_dir() {
            copiar(&p, &d);
        } else {
            std::fs::copy(&p, &d).unwrap();
        }
    }
}

#[test]
fn sync_e_relatorio_sobre_fixture() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("proj");
    std::fs::create_dir_all(root.join("proj_archive")).unwrap();
    let cp = d.path().join("projects");
    let pasta = cp.join(slug(&root));
    copiar(Path::new("tests/fixtures/cost"), &pasta);
    // o cwd anonimizado da fixture aponta pro projeto de mentira deste teste
    for arq in [pasta.join("sessao-1.jsonl"), pasta.join("sessao-1/subagents/agent-ag1.jsonl")] {
        let t =
            std::fs::read_to_string(&arq).unwrap().replace("/FIXTURE/proj", root.to_str().unwrap());
        std::fs::write(arq, t).unwrap();
    }
    let p = Projeto { root: root.clone(), archive: root.join("proj_archive") };
    let env = Ambiente {
        claude_projects: cp,
        pricing_usuario: None,
        agora: "2026-10-31T20:00:00Z".parse().unwrap(),
    };
    let tz = FixedOffset::west_opt(3 * 3600).unwrap();

    let r = sincronizar(&p, &env, &tz).unwrap();
    assert_eq!((r.novas, r.avisos.linhas_invalidas), (4, 1));
    assert_eq!(sincronizar(&p, &env, &tz).unwrap().novas, 0);

    let c = relatar(&p, &env, &tz).unwrap();
    let j = c.json();
    // msg_A conta UMA vez, com 200 de output (a maior linha do streaming)
    assert_eq!(j["total"]["respostas"], 4);
    assert_eq!(j["total"]["tokens"]["out"], 200 + 50 + 50 + 100);
    // 01:00Z do dia 30 é 22:00 do dia 29 em UTC-3
    assert_eq!(j["por_dia"][0]["dia"], "2026-10-29");
    // subagent soma na sessão do pai
    let sessao =
        j["por_sessao"].as_array().unwrap().iter().find(|s| s["sessao"] == "sessao-1").unwrap();
    assert_eq!(sessao["respostas"], 4);
    assert_eq!(j["por_agente"][0]["agente"], "ag1");
    // modelo sem preço: null + aviso, e o total parcial segue calculado
    let sem = j["por_modelo"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["modelo"] == "claude-modelo-futuro-9")
        .unwrap();
    assert!(sem["usd"].is_null());
    assert!(j["total"]["usd"].is_number());
    assert!(j["avisos"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a.as_str().unwrap().contains("sem preço")));
    assert!(p.custos().md().is_file() && p.custos().json().is_file());
}
