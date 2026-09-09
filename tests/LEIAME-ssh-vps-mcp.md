# Por que os testes de `ssh`, `vps` e `mcp` não estão mais aqui

Estes testes e2e viviam neste repo e foram **removidos**, não perdidos: cada um existe,
idêntico, no `schematize_deployer_rs`.

**O motivo é que eles pararam de testar algo daqui.** Eles invocavam o binário `schematize`
com `mcp …` / `vps add …`, e desde o ADR-0010 (enfim cumprido) esses comandos são
**encaminhados** ao `schematize-deployer`. O que eles exercitavam passou a ser o binário do
outro repo — na minha máquina passavam, porque o deployer está instalado; no CI, que não tem o
deployer, reprovaram em bloco.

Um teste que só passa quando outro app está instalado não testa este repo: testa a máquina de
quem roda. E foi assim que ele reprovou — depois do push, não antes.

**O que ficou:** `vps_conformidade.rs`, que lê o FONTE dos módulos `vps`/`sshkeys` — e esses
módulos continuam aqui enquanto a GUI os consome. Quando a GUI delegar, os módulos saem e esse
teste vai junto.
