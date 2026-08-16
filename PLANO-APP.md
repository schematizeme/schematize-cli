# Plano do app `schematize` — reorg + features (2026-08-16)

Feedback do usuário → redesenho do app. O **software é uma coisa**; as **skills são só
UMA funcionalidade** (a primeira, não a única). Convenção do checklist (pedida pelo user):
- 🤖 **check de máquina** — o agente/eu faço e marco.
- 👤 **check de humano** — só o humano fecha (validar visual, decisão, teste manual).

GUI = crate **Slint** (`schematize_gui_slint`, já é o default). CLI/engine = `schematize_cli_rs`.

---

## FASE 1 — Fundamentos (separação + tela inicial + barra + bug do restart)

### 1.1 Separar APP × SKILLS na CLI
- 🤖 `schematize upgrade` = **só o app** (self-update do binário). (já é isso; deixar explícito no help/doc)
- 🤖 `schematize skills <install|update|remove|list> [--all] [--with-recommended]` = gestão de skills (mover pra subcomando `skills`).
- 🤖 Manter os antigos `schematize install|update|list|remove` como **aliases ocultos** (compat, não quebrar scripts).
- 👤 Confirmar a nomenclatura final (`skills update` vs `skills upgrade`).

### 1.2 Tela inicial (launcher) na GUI — abrir escolhendo o que fazer
- 🤖 A GUI **não abre mais direto nas skills instaladas**. Abre numa **Home**: cards grandes → **Overdev** · **Mercado de Skills** (+ espaço p/ futuros: environments, ssh, settings).
- 🤖 "Atualizar skills" vira função **secundária** (dentro do Mercado, aba Instaladas).
- 👤 Validar o visual da Home.

### 1.3 Barra de topo enxuta em Overdev e Grafo (o resto vai pra modal)
- 🤖 Uma linha só, nos DOIS: `Projeto: [selector] · Abrir pasta · Abrir no VSCode · ⟳(reload, label no hover) · Diretórios de dev`.
  - **Abrir pasta** → abre a pasta do projeto no gerenciador de arquivos (`xdg-open <root>`).
  - **Abrir no VSCode** → `code <root>` (ou `vscode://file/<root>`).
  - **⟳ Reload** → ícone de reload; "reload" aparece no hover.
  - **Diretórios de dev** → abre **MODAL** de gestão dos dev_dirs (add via picker / remover) — hoje polui a tela listando tudo inline.
- 👤 Validar as duas barras + o modal.

### 1.4 BUG: "Reiniciar" só fecha, não reabre
- 🤖 O `restart` pós-update deve **relançar a janela nova** (spawn detached do binário atualizado, depois sair). Corrigir no Slint (e no egui fallback).
- 👤 Confirmar que reabre atualizado.

---

## FASE 2 — Mercado de skills: gerenciar + criar + editar
- 🤖 No Mercado, aba/seção **Gerenciar**: update/instalar/remover (já existe) + **Criar skill** + **Editar skill**.
- 🤖 **Editar skill**: abrir uma skill instalada (SKILL.md + references + commands), editar no painel, **aplicar a versão modificada localmente** (grava em `~/.claude/skills/schematize-<slug>/`).
- 🤖 **Criar skill**: scaffold de skill nova do zero (estrutura da casa: SKILL.md/refs/commands/skill.toml/VERSION), no painel; casa com a skill `schematize-scaffold`.
- 👤 Onde publicar a skill criada (repo/local) — decisão.

---

## FASE 3 — Overdev: editor + gestor de tasks + checklist 2-níveis (a parte pesada)
- 🤖 **Checklist de 2 checks** (mudança no engine `overdev.rs` + formato + UI): `- [ ]`/`- [x]` **máquina** (o agente pode mexer) e um novo tipo **humano** (ex.: `- [H ]`/`- [H x]`) que **só o humano** fecha manualmente pela interface (CLI ou GUI). O overdev não conclui enquanto houver humano aberto (mas não tenta fazer sozinho).
- 🤖 **Editor de texto acoplado + gestor de tasks** dentro do Overdev: ver/editar o PLAN/CHECKLIST, escrever **prompts pra corrigir o overdev** (se em desacordo) e **pontos específicos por task** pré-gerada.
- 🤖 Persistir esses prompts/pontos no control-plane do overdev (`.overdev/`).
- 👤 Usar o editor pra refinar um overdev real.

---

## FASE 4 — "Executar overdev" com agente acoplado (ambiciosa)
- 🤖 Botão **Executar overdev** → abre um **CLI do Claude (ou GPT)** acoplado, que roda o overdev e a app **acompanha**: se o agente **pausar**, manda `continue` + a **lista específica** do que revisar.
- 🤖 Integração: o schematize já tem hooks de overdev (Stop/PreToolUse). Estender pra a GUI **disparar e monitorar** uma sessão de agente (spawn do `claude`/CLI, ler estado do `.overdev/`, reenviar continue).
- 👤 Escolher o agente (Claude Code CLI vs outro) e autorizar a automação.
- ⚠️ Design a detalhar (é a mais complexa; provável spike antes).

---

## Ordem de ataque
Fase 1 agora (2 frentes paralelas: CLI restruct + Slint GUI). Fases 2–4 em sequência, cada uma
revisada por você. As 🤖 eu fecho; as 👤 dependem de você abrir o app e validar.
