# Estúdio vocal

V0.2 de uma bancada web para praticar canto com uma faixa alvo e acompanhamento opcional. Arraste seus arquivos ou abra a demonstração sintetizada de 24 segundos.

A voz alvo vira um piano roll. A voz captada aparece por cima em âmbar, com nota, frequência, desvio em cents, intervalo seguinte e pontuação. Mixer com volumes/mute separados, seek, loop A–B e atalhos de transporte.

## Rodar

Node.js 22.12+ e npm.

```sh
npm ci
npm run dev
```

Abra o endereço indicado pelo Vite. Microfone requer localhost ou HTTPS. Para verificar e gerar o site estático:

```sh
npm test
npm run build
npm run preview
```

O build fica em `dist/`. Nenhum servidor de aplicação, banco ou chave de API é necessário. A configuração de hospedagem privada do Sites está em `.openai/hosting.json`.

## Uso

1. Escolha o conteúdo do alvo: uma voz isolada ou várias vozes/notas. Carregue a faixa alvo. A backing track é opcional; pode ser removida. Quando presentes, os dois arquivos começam em 00:00; exporte stems com a mesma origem temporal.
2. Abra a bancada. No modo polifônico experimental, confira as candidatas cinza, escolha uma linha por registro/critério agudo ou grave, ouça e confirme a linha verde antes de pontuar. Essa seleção não identifica naipes nem separa o áudio. Clique na forma de onda ou no gráfico para escolher o ponto de início.
3. Ative a entrada e permita o microfone. Escolha a interface e um dos canais que o navegador disponibilizar.
4. Ajuste os volumes. O retorno de microfone começa desligado; use fones se precisar ligá-lo, ou o retorno direto da interface.
5. Use Espaço para play/pause e Enter para voltar ao início. Atalhos não interceptam campos nem botões. A/B definem o trecho repetido; as setas também posicionam os gráficos.

O acerto representa o tempo de alvo dentro da tolerância selecionada (25, 50 ou 75 cents). Cobertura representa presença de voz nesse tempo. Silêncio no alvo não pontua; silêncio do cantor durante o alvo conta como erro. Uma nota na oitava errada não é acerto. Seek, troca de faixa, loop e ajuste da tolerância/compensação reiniciam a tentativa. Mudar a seleção polifônica também invalida a revisão. A avaliação reserva a janela/compensação de captura e 80 ms de processamento antes de contar ausência como erro; a cauda sem captura ao parar ou repetir não é avaliada. Pause/resume preserva os totais e inicia um novo segmento sem avaliar o intervalo excluído. O painel informa tempo efetivamente avaliado e margem; loops menores que a margem mais 100 ms não pontuam.

## Limites da v0

- YIN no microfone e em alvos de uma voz, aproximadamente C2–D6 (65–1200 Hz), referência A4=440 Hz. Bleed, reverb e harmônicos fortes podem produzir erros ou saltos de oitava.
- Basic Pitch local estima notas simultâneas no modo polifônico experimental. Não recupera identidade de cantor/naipes nem garante todas as notas. A linha revisada usa centros de notas MIDI, não a afinação contínua exata de cada cantor da gravação. Apenas C2–D6 entra na avaliação; outras candidatas continuam visuais.
- A análise neural usa Workers descartáveis em blocos de 20 segundos, até 0,5 s de contexto por lado, timeout de 120 s por bloco e cancelamento. Arquivos longos podem levar minutos. Pesos e licença do modelo estão em public/models/basic-pitch; não há download de modelo de terceiros durante o uso.
- O benchmark real atual mediu RPA ±50 cents de 84,56% em 40 clips Vocadito. Em três misturas artificiais de duas vozes, Basic Pitch obteve F1 de 67,89%. Não é transcrição profissional nem validação com coro de vários naipes gravado junto.
- Até 80 MiB e 15 minutos por arquivo. WAV, MP3, M4A, OGG, AAC e FLAC dependem dos codecs do navegador; o app informa erros de decodificação.
- Não há upload para servidor: arquivos são decodificados e analisados localmente em memória. Recarregar a página exige importar novamente. Não há gravação, histórico ou separação de stems.
- Seleção de entrada/canais usa somente o stream exposto pelo navegador. Não existe controle web do buffer ASIO/CoreAudio: configure-o no driver/sistema. A janela de análise ajusta detecção, não o buffer do driver.
- Compensação manual aplica-se ao relógio da captura antes do mapeamento do loop. Valores positivos alinham uma entrada atrasada. A janela do detector já é descontada pelo timestamp do centro da captura. Latência de grafo/saída é informada pelo navegador e não mede o percurso completo do microfone.
- Seleção de saída depende de `AudioContext.setSinkId`; quando não há suporte, vale a saída do sistema. Chrome/Edge desktop recentes são o caminho principal para interfaces de áudio; Safari/Firefox podem expor menos controles.

## Organização

- `src/audio/engine.ts`: relógio único, fontes sincronizadas, loop, mixer e lifecycle da entrada.
- `src/audio/pitch.ts`: detector YIN, resampling, segmentos e waveform.
- `src/audio/*.worker.ts`: análise de arquivos e pitch ao vivo fora da UI.
- `public/pitch-capture.js`: AudioWorklet, ring buffer e timestamps da captura.
- `src/useStudio.ts`: estado e ações da sessão.
- `src/practice.ts`: pontuação, busca temporal e apresentação musical.
- `src/audio/polyphonic*`: inferência neural, ciclo de vida dos Workers, chunks e candidatos simultâneos.
- `src/target.ts`: escolha de uma linha de estudo, limite avaliável e gate de revisão.
- `src/components/`: piano roll e waveform em canvas, importação, revisão do alvo e configurações de áudio.
- `src/App.tsx`: fluxo de importação, transporte, mixer, entrada e feedback.

[Spec](docs/SPEC.md), [contrato de áudio](src/audio/README.md) e [validação](docs/VALIDATION.md).

## Pesquisa e evidências

[Mercado: 12 produtos](docs/RESEARCH-MARKET.md), [papers, modelos e protocolo](docs/RESEARCH-AUDIO.md), [benchmark monofônico com 40 clips](docs/BENCHMARK.md), [benchmark polifônico exploratório](docs/BENCHMARK_POLYPHONIC.md). Os scripts reproduzíveis estão em `scripts/`; os áudios dos corpora ficam fora do repositório.
