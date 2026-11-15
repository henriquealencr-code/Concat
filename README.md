# Berseker

Editor de vídeo especializado para **Windows x64**, baseado no
[Concat](https://github.com/jub0t/Concat). Prioridades: estabilidade, leveza,
precisão e desempenho. Interface padrão em português brasileiro.

**Versão de validação: ainda não declarada estável.** O problema do Enhance e a
fluidez do Preview precisam ser retestados na Intel UHD Graphics 620 em que a
falha foi reproduzida.

O editor mantém múltiplas faixas de vídeo/áudio, cortes e junções, colagem na
faixa original, reversão e loops, aprimoramento por IA até UHD 4K, interpolação
real para 60 FPS, volumes e exportação MP4. O estado das implementações, os
limites e os testes efetivamente executados estão no
[registro Windows do Berseker](docs/BERSEKER_WINDOWS.md).

O Enhance roda em processo isolado, usa DX12 por padrão no Windows e tenta CPU
se o processo de GPU falhar. O Preview possui cache, proxies e resolução
adaptativa; a exportação usa os originais. Instalação e dados são separados dos
do Concat. O formato de projetos existente foi mantido.

## Baixar a versão de validação

Na [página de Actions](https://github.com/henriquealencr-code/Concat/actions),
abra uma execução **Windows x64** concluída com sucesso para a branch `work`.
Em **Artifacts**, baixe `Berseker-windows-x86_64`, extraia o ZIP e use o
`Berseker-*-setup.exe` ou o `.msi`. O artefato exige uma sessão no GitHub e pode
expirar conforme a retenção do serviço. A documentação registra a execução
correspondente à entrega, quando confirmada.

## Desenvolvimento e licença

A implementação continua aproveitando os crates e as bibliotecas do Concat.
A documentação técnica de origem está preservada em
[UPSTREAM_README.md](docs/UPSTREAM_README.md) e no
[repositório original](https://github.com/jub0t/Concat); suas listas de ferramentas
não descrevem o escopo reduzido da interface Berseker.

Copyright original de Jareer Abdullah e colaboradores. Licença
[AGPL-3.0-or-later](LICENSE). Mantidos os
[avisos de terceiros](THIRD_PARTY_NOTICES.md). Os instaladores incluem esses
arquivos e a tela Sobre atribui a origem ao Concat.
