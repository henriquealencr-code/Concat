# Editor especializado para Windows x64

Prioridades: estabilidade, leveza, precisão e desempenho. O produto deve conter
somente as funções solicitadas e as dependências internas necessárias. Este
registro distingue a primeira etapa implementada do editor final, ainda incompleto.

## Etapa 1: Enhance

Foram encontrados caminhos concretos de falha, mas **o fechamento original não foi
reproduzido no Windows com a GPU do usuário**:

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

A [execução Windows x64 #1](https://github.com/henriquealencr-code/Concat/actions/runs/43691660160)
passou: regressões do Enhance (incluindo modelo real e UHD), compilação do aplicativo,
smokes do pacote Windows, geração dos instaladores e upload do artefato
`Concat-windows-x86_64`. O pacote contém o `.exe` de instalação e o `.msi`.
Essa versão corresponde ao commit `cb63fccb`, anterior às novas funções abaixo.
A compilação Rust da janela também passou neste Linux (`cargo check -p concat
--lib --profile test`), além da compilação Slint dos novos diálogos.

O runner não substitui testes em máquinas Windows com GPUs Intel, NVIDIA e AMD,
pouca VRAM/RAM e projetos não salvos. O fechamento original do Enhance permanece
sem reprodução/validação na máquina afetada. Os testes de modelo no runner não
significam teste interativo do Enhance no executável com uma GPU física.

## Novas funções e uso

- **Colar na faixa original:** o destino é identificado pelo ID da faixa, não
  pela posição visual. A âncora escolhe somente o tempo. Faixa removida ou
  bloqueada recusa o grupo inteiro, sem colocá-lo numa vizinha. Dois testes
  verificam reordenação, espaçamento, bloqueio e faixa inexistente.
- **Repetir sequência:** selecione clipes contíguos numa mesma faixa, abra o menu
  do clipe e escolha **Repetir sequência…**. Informe o total de ciclos, incluindo
  o original. A operação conserva os arquivos e usa referências na linha do
  tempo; recusa sobreposição, mais de 100 mil clipes, mais de 100 horas ou uma
  estimativa excessiva de metadados. Undo/redo e salvar/abrir foram testados com
  30 mil repetições. Fades e movimentos já existentes nos clipes são preservados.
- **Ida e volta:** duplique o trecho, reverta a cópia e encoste os dois na mesma
  faixa. Para não repetir as imagens das viradas, retire um quadro de cada ponta
  do trecho revertido e volte a encostá-lo no original. Selecione o par e repita.
  A regressão exercita esse fluxo e verifica os 952 quadros de duas voltas.
  Não há detecção automática de que dois arquivos formam um par reverso.
- **Interpolar para 60 FPS…:** no menu de um clipe de vídeo em velocidade normal,
  gera uma cópia MP4 com compensação de movimento bidirecional (`minterpolate`,
  modo MCI/AOBMC) do FFmpeg já vinculado ao programa. Não é mera duplicação ou
  mistura de quadros. A troca da mídia e a mudança da linha do tempo para 60 FPS
  são uma única etapa de desfazer. Há progresso, cancelamento, arquivos parciais,
  verificação de memória e rejeição de origem modificada durante o trabalho.
  Um teste de quadrado em movimento verifica posições intermediárias novas.
  Testes de vídeo conferem 240 quadros em quatro segundos, áudio e original
  intactos, cancelamento e mudança da origem. A conversão processa o arquivo
  de mídia inteiro, mesmo quando o clipe está aparado; o in-point é preservado.
- **Áudio em várias faixas:** o motor existente foi preservado. A regressão de
  duas músicas sobrepostas mede a soma dos ganhos, redução de volume e mute de
  uma faixa sem afetar a outra. Não foi acrescentada outra biblioteca de áudio.

Limites atuais da interpolação: fonte SDR com uma faixa de áudio interna,
velocidade normal sem curva, menos de 60 FPS, dimensões pares entre 16 pixels e
UHD 4K. HDR/gamut amplo e múltiplos streams de áudio internos são recusados para
não converter cores ou descartar áudio silenciosamente. Isso não limita a
quantidade de músicas/faixas independentes da linha do tempo. Interpolação é
executada na CPU; o encoder de vídeo tenta usar o hardware disponível. O modelo
não garante movimentos sem artefatos: o resultado depende da cena.

## Regressão e entrega

As verificações locais incluem 256 testes de projeto, interpolação de movimento,
vídeo/áudio/cancelamento, arquivos de origem modificados, loops longos e recusa de
metadados excessivos, ida e volta, cortes fracionários e junções entre arquivos de
FPS diferentes. Foram conferidos todos os 720 quadros de três repetições simples,
os 840 quadros de uma sequência de dois arquivos repetida e os 952 da ida e volta.
As fronteiras são verificadas sem quadros pretos nem deslocamento das imagens.
Isso não equivale a medir a reprodução interativa em todos os computadores.

O workflow Windows agora inclui essas regressões antes de compilar o pacote. O
push na branch `work` inicia **Windows x64** automaticamente. Para baixar uma
execução concluída, abra **Actions → Windows x64 → execução → Artifacts →
Concat-windows-x86_64**, extraia o ZIP e execute o `*-setup.exe` ou use o `.msi`.
A geração da versão com todas as novas funções ainda precisa ser confirmada na
execução correspondente ao commit dessas alterações.

Permanecem pendentes: validação interativa no Windows/GPU físico, medidas do
pacote final e consumo em 4K60 de longa duração, casos VFR/HDR/multi-stream da
interpolação e a remoção dos painéis/dependências antigos fora do escopo pedido.
Nenhuma dependência de produção, ferramenta ou efeito não solicitado foi adicionado.
O Enhance **não é considerado resolvido na máquina afetada**.
