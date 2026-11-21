# BUDGET.md - orçamento de RSS do `schematize` (CLI)

Piso: `skill_engineering_src/references/eficiencia-runtime.md`. Teto = medido depois + ~30% de folga
(arredondado), sempre <= 256 MiB para CLI. Medição: `VmHWM` de `/proc/<pid>/status` (pico de RSS).

| Operação | RSS pico alvo (teto) | Antes | Depois | Como medir |
|---|---|---|---|---|
| `schematize --version` | 16 MiB | < 2 MiB (não amostrável: sai em < 2 ms) | idem | `cargo test --test orcamento_rss version_dentro_do_orcamento` |
| `schematize status` | 16 MiB | 4,5 MiB (binário instalado, release) | 9,4 MiB (build debug do teste) | `cargo test --test orcamento_rss status_dentro_do_orcamento` |
| `schematize doctor` (GUI de 96 MiB no HOME) | 16 MiB | 33,2 MiB com GUI real de 30 MB (lia o binário inteiro: `fs::read` x2) | 9,3 MiB (debug) | `cargo test --test orcamento_rss doctor_nao_carrega_gui_inteira_em_memoria` |

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
