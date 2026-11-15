# Enhance: leitura DX12 e recuperação na UHD 620

O erro `upscaler: the answer would not map` escapava da classificação de falhas
GPU do supervisor. Agora todos os erros do upscaler acionam a recuperação
isolada por CPU, descartam os parciais e mantêm CPU nas operações seguintes até
reiniciar o aplicativo. Falhas de entrada continuam sem essa classificação.

A leitura espera pela submissão exata que produz o buffer. Falhas de mapping
cancelam o mapeamento, invalidam a instância e registram o erro, a causa de
perda/validação capturada e o tamanho do buffer. Uma causa ausente é registrada
como ausente, sem assumir que houve falta de VRAM ou reset do dispositivo.

Na Intel em DX12, as convoluções são submetidas em lotes de até quatro camadas,
com conclusão limitada por prazo entre lotes, antes do shuffle e da cópia para
staging. Isso limita trabalho pendente e dá ao driver oportunidades de concluir
as submissões; não altera pesos, precisão, resolução nem camadas do modelo.
É uma mitigação a validar fisicamente, não uma prova de que o driver foi corrigido.

O progresso agora considera cada tile, inclusive no primeiro quadro. A interface
mostra duas casas decimais e transmite atualizações menores, sem esperar 1% do
vídeo inteiro. A CPU continua limitada para preservar responsividade; não há
promessa de inferência rápida de 1080p para 4K no i7-8650U.

Validação local: 16 testes de tiling/progresso passaram (dois testes com modelo
real permanecem ignorados nessa suíte); cálculo GPU em Vulkan por software,
com cinco camadas, conferiu igualdade entre lotes e caminho comum e tolerância
contra referência numérica; oito testes do supervisor passaram, incluindo a
mensagem exata de mapping seguida de inferência real CPU e preservação da origem.
Clippy das bibliotecas com `-D warnings` passou. A interface foi compilada com
Slint. Isso não valida DX12 nem o desempenho na Intel UHD 620: o reteste no
computador afetado continua obrigatório. Não foi acompanhado o GitHub Actions
para esta entrega.
