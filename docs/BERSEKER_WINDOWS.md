# Berseker para Windows x64 — versão de validação

**Não é uma versão declarada estável.** O fechamento relatado na Intel UHD 620
precisa ser retestado nessa máquina. O CI e uma GPU por software não substituem
esse teste nem medem a fluidez do Preview no computador afetado.

## Diagnóstico confirmado pelo log recebido

Windows 11 Pro, i7-8650U, 16 GB, Intel UHD Graphics 620, driver 31.0.101.2135.
O editor estava em Direct3D 12 e a decodificação chegou a usar Direct3D 11.
O Enhance verificou o modelo e abriu separadamente um adaptador Vulkan. Depois
registrou `0xC0000005`, escrevendo memória em `igvk64.dll+0x64ea72`. O registro
termina entre a descrição do adaptador e o registro dos buffers de inferência;
não identifica uma alocação ou shader específico. Não demonstra modelo inválido
ou esgotamento de RAM/VRAM. A inferência no mesmo processo permitia que a falha
do driver encerrasse a janela.

O Preview registrou esperas de até 2,11 s e centenas de quadros não apresentados.
A métrica antiga era calculada antes do upload/desenho: não separava fila,
decodificação e GPU. Portanto o log não basta para atribuir todas as pausas a uma
só dessas etapas.

## Mudanças

- Enhance e sua comparação antes/depois rodam em um processo separado do mesmo
  executável, iniciado antes de criar janela/GPU. Não há serviço residente extra.
  Uma falha nativa é detectada pelo processo pai. Há cancelamento que encerra e
  recolhe o filho, prazo sem progresso e logs persistentes para cada tentativa.
- O Windows usa **somente DX12 por padrão para a inferência**, sem voltar
  automaticamente a Vulkan. `WGPU_BACKEND=dx12` ou `vulkan` é respeitado pelo
  upscaler. O compositor independente também respeita essa restrição. A janela
  compartilhada com Skia exige a API de sua plataforma (DX12 no Windows).
  `WGPU_DX12_COMPILER` continua explícito; o padrão é FXC do Windows.
- Se a tentativa isolada de GPU morrer ou perder o dispositivo, há uma única
  nova tentativa em processo isolado com ONNX **CPU**, sem provider de GPU,
  até quatro threads e codificação por software (sem voltar ao driver no encoder). As operações seguintes usam CPU até reiniciar o aplicativo,
  evitando novas tentativas na GPU que falhou. Para diagnóstico, `BERSEKER_ENHANCE_DEVICE=cpu` evita abrir
  GPU desde o início. `CONCAT_GPU=software` também impede a inferência em GPU.
  Não há troca silenciosa para outra API gráfica após uma escolha explícita.
- No Windows o filho fica em um Job Object: encerra com o pai e tem limite de
  memória comprometida de até metade da RAM disponível, limitado a 4 GiB.
  Isso não é uma reserva de RAM nem um limite de VRAM do driver.
- Resultados ficam em pasta temporária privada. Somente após saída normal e
  resposta completa o pai publica a cópia, sem substituir um destino existente
  (no Windows, `MoveFileW` sem substituição, sem depender de hardlinks).
  Uma tentativa por CPU descarta antes qualquer resultado parcial de GPU.
  Arquivos de origem não são escritos. Mudança de tamanho/data na origem recusa
  o resultado. Cancelamento antes de iniciar o worker também é preservado.
- O projeto continua com sua mídia anterior quando o job falha. Cópias já
  concluídas no cache e o salvamento/recuperação existentes permanecem. Há
  checkpoints de progresso no log; um MP4 incompleto não é adotado nem retomado
  como se estivesse completo. Retomada da inferência quadro a quadro não foi
  implementada.
- Preview reduz dinamicamente de Full/Half até Quarter após quatro quadros
  atrasados; conta também upload/desenho. Pausado volta à resolução escolhida.
  Mudança de projeto/qualidade limpa a adaptação. Proxies são ativados nas
  configurações de uma instalação nova; uma escolha salva de desligá-los é
  respeitada. O proxy UHD é 960 × 540, mantém FPS e é escrito no cache em segundo
  plano. Só a reprodução e as miniaturas o usam; a exportação lê o original.
- A preparação de prefetch tem no máximo uma tarefa pendente. Logs de quadros
  lentos agora separam espera na fila e decodificação. Foi retirada a compilação
  antecipada de 85 efeitos que não fazem parte deste editor. Efeitos de projetos
  legados continuam sendo interpretados quando esses projetos são abertos.
- Tela cheia ganhou comando próprio, apresentação com proporção preservada e
  foco de teclado para Esc/Space. Usa a mesma imagem e o mesmo transporte; não
  inicia outra reprodução. Esc restaura o foco da edição.

## Identidade e escopo

Nome, recursos do executável, janela, tela inicial, ícone, atalhos e instaladores
usam Berseker. O ícone deriva do logotipo enviado. O padrão de idioma é pt-BR;
o inventário pt-BR cobre todas as chaves atuais. Os avisos AGPL e de terceiros
são mantidos, e a tela Sobre atribui a origem ao Concat e seus autores.

Executável distribuído: `Berseker.exe`. Instaladores: `Berseker-*-setup.exe` e
`Berseker-*.msi`, no artefato `Berseker-windows-x86_64`. IDs de instalação, códigos
de upgrade, atalhos e dados `app.berseker.editor` são distintos dos do Concat.
Não há migração que mova/apague os dados de `app.concat.editor`. Projetos antigos
podem ser abertos pelo seletor existente e mantêm seu formato.

Foram retiradas entradas de catálogos de efeitos, texto, voz, templates e páginas
de automação/atualização do Concat. Bibliotecas e código de compatibilidade
legado ainda existem; esta etapa não equivale a concluir a redução de todas as
dependências herdadas. Nenhuma biblioteca de produção nova foi adicionada;
foram habilitadas APIs adicionais na dependência Windows já existente.

## Testes e limites da validação

Testes novos incluem aborto nativo após escrever parcial, limpeza e repetição,
cancelamento de processo bloqueado, cancelamento antes do início, prévia com
modelo real, aborto de GPU seguido de inferência real por CPU, seleção explícita
de backend e adaptação de resolução. Há teste UHD60 com 60 quadros de proxy,
comparação dos bytes da origem e teste de exportação com um proxy preto que
não pode contaminar a imagem nem o áudio exportados.

O workflow repete as regressões de edição/exportação e de Enhance, gera os dois
instaladores e testa a prévia de IA e um vídeo através do executável empacotado.
O teste HDR de proxy pode retornar sem executar quando não há encoder HEVC de
10 bits; isso não valida HDR. Testes antigos que retornam sem modelo também não
contam como inferência. Resultados do CI são registrados após a execução.

**Ainda obrigatório:** reteste de Enhance e Preview UHD60 na UHD 620; verificar
que o log agora mostra DX12 ou fallback explícito para CPU, que a janela continua
aberta em falhas e que o projeto reabre. Testar tela cheia interativamente no
Windows, instalação/desinstalação lado a lado com Concat e duração/consumo em
projetos longos. Não há garantia de recuperação de um reset de GPU que afete o
sistema inteiro.

Logs novos: `%LOCALAPPDATA%\app.berseker.editor\logs`. Os logs de cada tentativa
isolada terminam em `-gpu.log` ou `-cpu.log`; a mensagem de falha informa o caminho.
