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

**O que ficou até agora:** `vps_conformidade.rs`, que lia o FONTE dos módulos `vps`/`sshkeys`
— e esses módulos continuavam aqui enquanto a GUI os consumia.

**A GUI delegou, e os módulos saíram.** As telas de Chaves SSH e de Hosts do hub agora abrem a
janela do `schematize-deployer`, que tem as três (chaves, hosts, cofre). Com elas foram
`src/sshkeys/`, `src/vps/`, `src/mcp/` e o `vps_conformidade.rs`, exatamente como este arquivo
previu. As regras que o `vps_conformidade` afirmava seguem valendo — e seguem testadas, no
`schematize_deployer_rs`, sobre o código que de fato roda.

Restou só este arquivo, e ele fica: o histórico de por que um teste some é mais útil que o
silêncio de um diretório que encolheu.
