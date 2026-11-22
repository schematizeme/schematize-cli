# BUDGET.md - orçamento de RSS do `schematize` (CLI)

Piso: `skill_engineering_src/references/eficiencia-runtime.md`. Teto = medido depois + ~30% de folga
(arredondado), sempre <= 256 MiB para CLI. Medição: `VmHWM` de `/proc/<pid>/status` (pico de RSS).

| Operação | RSS pico alvo (teto) | Antes | Depois | Como medir |
|---|---|---|---|---|
| `schematize --version` | 16 MiB | < 2 MiB (não amostrável: sai em < 2 ms) | idem | `cargo test --test orcamento_rss version_dentro_do_orcamento` |
| `schematize status` | 16 MiB | 4,5 MiB (binário instalado, release) | 9,4 MiB (build debug do teste) | `cargo test --test orcamento_rss status_dentro_do_orcamento` |
| `schematize doctor` (GUI de 96 MiB no HOME) | 16 MiB | 33,2 MiB com GUI real de 30 MB (lia o binário inteiro: `fs::read` x2) | 9,3 MiB (debug) | `cargo test --test orcamento_rss doctor_nao_carrega_gui_inteira_em_memoria` |
| `schematize conectores list` | 16 MiB | n/d (comando novo) | 9,14 MiB (build debug do teste) | `cargo test --test orcamento_rss conectores_list_dentro_do_orcamento -- --nocapture` |

Notas de honestidade:
- "Antes" = binário instalado em `~/.cargo/bin` (0.68.1, release); "depois" = build debug do fonte (0.66.9 + correção).
  Debug costuma usar >= release, então o ganho real é maior ou igual. Os tetos valem para o debug do teste.
- O teto de 16 MiB para `status` é folgado (3,4x o release) de propósito: debug + variação de CI.
- Causa corrigida: `doctor` carregava o `schematize-gui` inteiro em memória só para procurar
  `slint`/`eframe`/`egui`. Agora `varredura::contem_agulhas` lê em trechos de 64 KiB.
- Sem mudança de dependências: `notify-rust` (zbus) já está no mínimo (`z` = zbus+serde+async-io; a
  alternativa `tokio` traria runtime a mais), `png` só tem features opcionais, `clap` derive é usado.
  `[profile.release]` intocado (idêntico entre repos).
- Não medido: RSS do agente residente (`schematize agent`), precisa de sessão D-Bus/display.

## `schematize conectores` (ADR-0028, lib `schematize-connectors` v0.1.0)

- Delegação total à lib (`src/conectores.rs`); nada de lógica reimplementada. Dependência nova só da
  lib, que usa `serde`, `serde_json` e `clap` (já presentes no hub); rede só na prévia `--ao-vivo`, via `curl`.
- Tamanho do binário (release, strip, lto thin, mesmo perfil): 5.729.768 B sem a lib -> 6.134.760 B com ela
  (+404.992 B, ~395 KiB, +7,1%). Catálogo, textos pt/en e CLI da lib (`texto.rs`) são o grosso.
- RSS de `conectores list`: 9,14 MiB no build debug do teste (`status` mede 9,4 MiB), teto 16 MiB. O RSS do
  release não foi remedido (o binário release de medição foi descartado); debug costuma ser >= release.
- Decisão: sem feature opcional. +395 KiB não é relevante frente a um binário de 5,7 MB e o RSS não se
  move; uma feature exigiria chaves i18n em 20 idiomas e `cfg` na superfície travada. Se o catálogo crescer
  (mais conectores/idiomas), reabrir: expor `conectores` como feature `default` desligável.
- Teste de orçamento: `conectores_list_dentro_do_orcamento` em `tests/orcamento_rss.rs`.
