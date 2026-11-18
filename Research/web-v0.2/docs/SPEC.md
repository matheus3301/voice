# Estúdio vocal — spec v0.2

Status: v0.2 implementada e validada conforme docs/VALIDATION.md. Data: 2026-11-16.

## Objetivo

Estudar uma linha vocal sobre uma backing track, visualizando a melodia alvo e a voz captada em tempo real. Uma ferramenta de estudo, sem landing page, conta, slogan ou backend.

## Fluxo

1. Escolher explicitamente se o alvo tem uma voz ou várias notas; arrastar ou selecionar o alvo obrigatório e, opcionalmente, uma base. A base pode ser removida. Trocar arquivo, mostrar duração/estado/erros. WAV, MP3, M4A, OGG e FLAC quando decodificáveis pelo navegador. Limites de memória explícitos.
2. Decodificar localmente. Analisar o alvo fora da thread da interface, com progresso e cancelamento. Modo monofônico usa YIN; modo polifônico usa Basic Pitch e retorna todas as notas candidatas simultâneas. Extrair segmentos de notas e forma de onda de ambas as faixas. Os arquivos nunca são enviados.
3. Entrar na bancada: transporte no topo; faixas e panorama temporal; piano roll com notas alvo, curva original e curva ao vivo; mixer e entrada lateral.
4. Play/pause, clique na régua para posicionar, voltar ao início, loop A–B e zoom temporal. Tracks compartilham um relógio AudioContext e começam sincronizadas. Faixas menores terminam em silêncio. Espaço alterna reprodução; Enter volta ao início. Campos/formulários preservam suas teclas.
5. Ativar microfone por ação explícita. Escolher dispositivo, canal exposto, janela de análise, ganho/retorno e compensação de latência. Retorno desligado por padrão; recomendar fones ao ligá-lo. Seleção de saída somente se suportada.
6. Mostrar nota cantada, nota alvo, desvio em cents, direção do desvio, intervalo até a próxima nota e pontuação da tentativa. Recomeçar/seek limpa a tentativa; loop inicia nova tentativa. Silêncio do cantor durante alvo conta como ausência, silêncio do alvo não entra na pontuação.

## Pitch e pontuação

- Detector autocorrelação/YIN compartilhado entre análise e captura. Confiança e gate RMS removem silêncio/ruído; interpolação melhora precisão. A4 = 440 Hz; MIDI = 69 + 12 log2(f/440).
- Uma voz isolada é precondição do modo monofônico. O modo polifônico é experimental: candidatas simultâneas cinza, linha de estudo mint, critério agudo/grave dentro de faixa escolhida, revisão explícita obrigatória antes de pontuar. Não atribuir identidade de naipe/cantor. Mudar a seleção invalida revisão e tentativa. Centros MIDI da linha revisada são a referência, não o F0 contínuo de cada cantor original. A seleção avaliada respeita C2–D6; candidatas fora do detector permanecem apenas visuais.
- Alvo interpolado/amostrado por tempo; tolerância inicial ±50 cents. Nota em outra oitava é erro real. Score ponderado pela duração do alvo e acerto dentro da tolerância. Cobertura indica tempo cantado/tempo alvo. Exibir score apenas quando há evidência suficiente e mic ativo.
- Compensação manual de latência em milissegundos aplicada ao instante da captura para alinhar ao alvo; não altera áudio dos arquivos. Sem alegar calibração automática.

## Arquitetura

React + TypeScript + Vite, SPA estática. Engine Web Audio independente de React; pitch worker para arquivos e Basic Pitch em Workers descartáveis por blocos de 20 s, com 0,5 s de contexto e timeout120 s; AudioWorklet para captura. Componentes canvas para piano roll/formas de onda, React para controles e estado de sessão. Sem chamadas de rede para áudio. Demo sintetizada no código para testar sem material protegido. Backend, persistência de arquivos, time stretch, gravação, detecção polifônica e separação de stems ficam fora da v0.

## Restrições reais de hardware

getUserMedia só acessa canais/dispositivos apresentados pelo navegador/SO. Selecionar canal usa split do stream disponível, não oferece acesso irrestrito às entradas físicas. latencyHint solicita baixa latência e pode ser ignorado. Janela de análise é ajustável; buffer ASIO/CoreAudio é configurado no driver/SO. Mostrar sample rate e base/output latency quando disponíveis. Secure context (localhost/HTTPS) e permissão do usuário são necessários para microfone. Saída selecionável por AudioContext.setSinkId somente quando suportado.

## Direção visual

Bancada técnica de prática vocal. Domínio: fita, pista, medidor, afinação, intervalo, região de loop. Cores: grafite da mesa, branco fosco de teclas, verde do alvo, âmbar da voz, vermelho de clipping. Assinatura: linha da voz âmbar atravessando cápsulas verdes sobre teclado vertical e cursor. Evitar cards métricos genéricos, gradientes decorativos e telas de marketing. Controles densos com labels claros, foco de teclado visível, estados vazios úteis. Desktop primeiro; largura menor permite recolher painel sem perder transporte.

## Aceite e validação

- Importar duas faixas, ver notas extraídas e ouvi-las sincronizadas com volumes independentes/mute.
- Pausar/retomar/seek/Enter/loop sem sources duplicados ou desvio de relógio.
- Microfone mede pitch, retorno começa desligado e streams anteriores são encerrados ao trocar/desligar.
- Cents e score com silêncio, acerto, desafinação, outra oitava e intervalos verificados com sinais sintéticos.
- Worker não bloqueia renderização. Falha/cancelamento de importação não substitui sessão funcional por estado parcial.
- Build TypeScript, testes de DSP/tempo/score e teste no navegador: demo, upload real sintético, transportes, configuração, erros de arquivo e mic simulado. Captura física depende de teste com a interface do usuário.

## Referências

- Yousician, configurações do aplicativo: https://support.yousician.com/hc/en-us/articles/203751452-Game-tab
- Web Audio API: https://developer.mozilla.org/en-US/docs/Web/API/Web_Audio_API
- MediaDevices.getUserMedia: https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia

A experiência de prática e repetição foi conferida em: https://support.yousician.com/hc/en-us/articles/360000553289-Practice-and-Play-modes-in-singing

## Pontuação parcial e evidência da v0.2

O denominador aguarda meia janela de captura + compensação positiva + 80 ms de processamento. Callbacks válidos preenchem os bins recebidos. Caudas ainda não capturadas na parada/virada não são consideradas erros; ficam fora do tempo avaliado mostrado no painel. O início é limitado ao start/seek/ponto A. Pause/resume inicia segmento sem preencher a cauda excluída. Loops menores que margem+100 ms não pontuam. Esta margem de engenharia não é uma medição de latência física.

A implementação funcional e a precisão do estimador são avaliadas separadamente. Vocadito40 inteiro: RPA50c84,56%; três misturas artificiais de duas vozes: F1multi-F0 67,89%. Não alegar garantia de transcrição de backing vocal/coro. Manter casos de falha, parâmetros e hashes, e ampliar com corpus coral independente antes de recomendar detecção automática de naipes.

Referências de pesquisa: [mercado](RESEARCH-MARKET.md), [MIR e algoritmos](RESEARCH-AUDIO.md), [benchmark mono](BENCHMARK.md), [benchmark poly](BENCHMARK_POLYPHONIC.md).
