# Editor especializado para Windows x64

Prioridades: estabilidade, leveza, precisão e desempenho. O produto deve conter
somente as funções solicitadas e as dependências internas necessárias. Este
registro distingue a primeira etapa implementada do editor final, ainda incompleto.

## Etapa 1: Enhance

Foram encontrados caminhos concretos de falha, mas **o fechamento original não foi
reproduzido no Windows**:

- O compositor já força FXC porque o DXC encontrado no PATH pode carregar uma
  `dxcompiler.dll` incompatível e causar uma falha nativa. O upscaler usava a
  configuração padrão de wgpu. Agora ele segue a mesma escolha FXC e os mesmos
  limites de orçamento de memória do compositor, preservando o override explícito
  usado para diagnóstico.
- O upscaler não tinha handler para erros não capturados nem perda do dispositivo.
  O handler padrão de wgpu pode produzir panic; o perfil `app` usa `panic=abort`.
  Agora erros são registrados e devolvidos ao job; inicialização captura erros de
  validação/VRAM, tamanhos de buffers são verificados e esperas têm prazo.
- A inferência criava uma imagem inteira em quatro vezes cada dimensão antes de
  reduzi-la. Agora tiles contribuem diretamente para o tamanho final. As alocações
  grandes do acumulador/saída são falíveis, e o host verifica uma estimativa
  conservadora de RAM disponível antes de carregar o modelo e a cada quadro.
- Um arquivo instalado era aceito apenas por existir e não estar vazio. O Enhance
  agora verifica tamanho e SHA-256 antes de carregar um modelo ainda não em cache.

Cancelamento é verificado entre tiles e antes de gravar uma imagem. Falhas não
adotam o resultado no projeto. A escrita existente sob nomes parciais permanece;
uma falha descarta o modelo carregado para permitir uma nova tentativa. Os logs
incluem adaptador, limites, buffers, modelo verificado, estimativa de RAM e erro do
job/preview. A interface existente apresenta os erros retornados pelo job.

O limite foi elevado a UHD 4K, 3840 × 2160 (ou orientação vertical equivalente).
Fontes acima desse limite são recusadas com erro, não ampliadas indefinidamente.
O modelo compacto continua limitado a ampliação máxima de 4×; fontes pequenas
não passam a produzir qualquer tamanho arbitrário. Nenhuma dependência de
produção, manifesto Cargo ou lockfile foi acrescentado/alterado.

Essas mudanças não isolam falhas nativas do driver em outro processo nem garantem
recuperação de um encerramento imposto pelo sistema operacional. A estimativa de
RAM não é uma reserva de memória pelo sistema. Testes Windows com GPUs reais e
pressão de memória continuam obrigatórios antes de declarar o problema resolvido.

## Validação executada neste Linux

- 15 testes do módulo de tiling: cobertura, bordas fracionárias, equivalência com
  o caminho anterior (diferença máxima de uma unidade de canal), dimensões
  inválidas, cancelamento e respostas inválidas/falhas do modelo.
- Um teste provoca erro real de validação wgpu e verifica retorno de erro sem
  panic, usando Vulkan por software.
- Testes do host rejeitam modelo corrompido com tamanho correto, recusam orçamento
  de RAM insuficiente e preservam mídia/projeto/cache após falha injetada.
- Real-ESRGAN real, SHA-256 verificado: inferência pequena e um quadro inteiro
  1920 × 1080 → 3840 × 2160; este último passou em aproximadamente 61 segundos na
  CPU desta máquina. Isso não é uma medida de desempenho Windows/GPU.
- Um vídeo real com quatro quadros foi aprimorado, gravado em MP4/H.264 e lido de
  volta: tamanho, contagem de quadros, progresso e original intacto verificados.
- Regressões existentes de cortes fracionários sem quadros pretos e de reversão
  de um trecho passaram usando o FFmpeg fixado pelo CI.
- `cargo fmt --check`, Clippy com `-D warnings` nas três bibliotecas alteradas,
  inventário de traduções e seleção de alvos do workflow foram verificados.

A primeira tentativa da regressão de cortes falhou porque o FFmpeg Debian não
oferecia `libopenh264`, o encoder procurado pelo Concat. O ambiente passou a usar
exatamente o FFmpeg LGPL fixado e verificado pelo CI; a regressão então passou.
Não foi habilitado x264 como contorno.

Os testes antigos do host `a_span_is_enhanced_on_its_own` e
`the_gpu_enhances_as_onnx_runtime_does` retornaram sem executar inferência por
procurarem um modelo num diretório macOS. Eles **não contam** como validação da
inferência nesta máquina. Foram acrescentados testes explícitos com caminho do
modelo, executados separadamente. Benchmarks ignorados também não contam.

## Build Windows x64

O workflow manual **Windows x64** em `.github/workflows/windows-x64.yml` chama o
empacotamento existente com somente `x86_64-pc-windows-msvc`, no Windows Server
2022. Mantém toolchain, checksums, bibliotecas e avisos de licença do projeto.
As chamadas de release existentes continuam selecionando os cinco alvos originais.

O job inclui as regressões do Enhance, baixa/verifica o modelo para os testes,
executa inferência real e o teste de vídeo, compila o perfil `app` e reaproveita os
smokes/instaladores existentes. O modelo de teste fica no diretório temporário do
runner, sem ser acrescentado ao pacote do aplicativo.

**O workflow não foi executado no GitHub nesta tarefa; nenhum `.exe` foi produzido
ou validado aqui.** Um runner hospedado também não substitui testes em máquinas
Windows com GPUs Intel, NVIDIA e AMD, pouca VRAM, pouca RAM, modelo corrompido,
rede interrompida, cancelamento e projetos não salvos. A janela não foi compilada
nem testada nesta máquina Linux; o fluxo validado aqui é motor/CLI/API.

## Requisitos e etapas seguintes

| Requisito | Evidência/estado atual |
| --- | --- |
| Importar, salvar/abrir, undo/redo, várias faixas e volumes | Implementações existentes; regressões de projeto/motor passaram na preparação. Interface Windows ainda sem validação. |
| Cortar, dividir, mover, duplicar, juntar | Implementações existentes; cortes fracionários e reversão foram verificados de ponta a ponta. Não representa validação de todos os gestos da interface. |
| Copiar/colar na faixa original | Pendente: o paste atual pode deslocar as faixas para acompanhar o clipe usado como âncora e pode buscar faixa desbloqueada vizinha. Deve preservar a faixa ou recusar explicitamente, com regressões. |
| Reversão e loops de ida e volta | Reversão existente e teste de trecho passou. Construção de loop/ida e volta e limites entre ciclos ainda pendentes. |
| Loops contínuos de longa duração | Pendente: verificar continuidade quadro a quadro, áudio, limites de duração, uso de disco e memória. |
| IA até 4K | Caminho implementado e testado na CPU Linux, incluindo quadro UHD e vídeo pequeno. Desempenho e estabilidade Windows/GPU pendentes. |
| Interpolação real para 60 FPS | Não implementada. Selecionar exportação em 60 FPS não cria quadros intermediários por movimento. |
| Múltiplas músicas e ajuste/sobreposição | Motor existente; regressões específicas de mixagem e testes da interface Windows ainda necessários. |
| MP4 H.264/HEVC até 4K60 | Exportação existente; H.264 exercitado no vídeo aprimorado. HEVC/4K60 completos no Windows ainda sem validação. |
| Progresso, cancelamento e recuperação | Tratamento do Enhance ampliado; cancelamento entre tiles testado no motor. Rede interrompida e pressão real de RAM/VRAM no Windows pendentes. |
| Interface simples em pt-BR e remoção de funções extras | Tradução existente preservada e limite 4K atualizado. Simplificação/remoção de painéis, ferramentas e dependências não solicitados ainda não executada. |
| Leveza e licenças | Nenhuma dependência de produção nova. Tamanho, consumo e desempenho do executável final ainda não medidos. Licenças e avisos originais preservados. |

Ordem de trabalho: concluir validação Windows do Enhance; corrigir semântica de
colar e implementar loops com regressões; implementar interpolação real e validar
4K60; restringir interface/dependências ao escopo pedido; executar regressão final,
medir recursos e gerar o executável. Cada etapa deve manter os projetos existentes
abrindo corretamente e preservar os arquivos originais. Não há autorização de
escopo para acrescentar outros efeitos ou ferramentas.
