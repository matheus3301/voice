# Estúdio vocal

V0 de uma bancada web para praticar canto sobre duas faixas: acompanhamento e voz alvo. Arraste seus arquivos ou abra a demonstração sintetizada de 24 segundos.

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

1. Carregue uma backing track e uma voz alvo monofônica e isolada. Os dois arquivos começam juntos em 00:00; exporte stems com a mesma origem temporal.
2. Abra a bancada. Clique na forma de onda ou no gráfico para escolher o ponto de início.
3. Ative a entrada e permita o microfone. Escolha a interface e um dos canais que o navegador disponibilizar.
4. Ajuste os volumes. O retorno de microfone começa desligado; use fones se precisar ligá-lo, ou o retorno direto da interface.
5. Use Espaço para play/pause e Enter para voltar ao início. Atalhos não interceptam campos nem botões. A/B definem o trecho repetido; as setas também posicionam os gráficos.

O acerto representa o tempo de alvo dentro da tolerância selecionada (25, 50 ou 75 cents). Cobertura representa presença de voz nesse tempo. Silêncio no alvo não pontua; silêncio do cantor durante o alvo conta como erro. Uma nota na oitava errada não é acerto. Seek, troca de faixa, loop e ajuste da tolerância/compensação reiniciam a tentativa.

## Limites da v0

- Análise monofônica, YIN, aproximadamente C2–D6 (65–1200 Hz), referência A4=440 Hz. Várias vozes, bleed, reverb e instrumentos misturados podem produzir erros ou saltos de oitava. Não é uma transcrição profissional.
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
- `src/components/`: piano roll e waveform em canvas, importação e configurações de áudio.
- `src/App.tsx`: fluxo de importação, transporte, mixer, entrada e feedback.

[Spec](docs/SPEC.md), [contrato de áudio](src/audio/README.md) e [validação](docs/VALIDATION.md).
