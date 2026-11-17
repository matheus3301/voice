# Benchmark polifônico — misturas artificiais de vozes reais

Executado em 2026-11-16. **Amostra de três misturas artificiais**, não um benchmark de coro natural nem o corpus completo. Resultado agregado nesta amostra com o pipeline de produção: **precisão 80.40%, recall 58.74% e F1 67.89%**, com tolerância de ±50 cents, usando notas MIDI centrais como aproximação de multi-F0. O pitch bend não foi incorporado.

## Fontes e licenças

As seis gravações são do [Vocadito, Zenodo 5578807 v3](https://zenodo.org/records/5578807), CC BY 4.0, de Rachel M. Bittner, Katherine Pasalo, Juan José Bosch, Gabriel Meseguer-Brocal e David Rubinstein. O [benchmark monofônico](BENCHMARK.md) registra a atribuição completa, MD5/SHA-256 do ZIP verificado e o protocolo do corpus inteiro. Áudios e anotações ficam em cache privado; as misturas só existem em memória.

O modelo e o pós-processamento vêm do [Spotify Basic Pitch TypeScript](https://github.com/spotify/basic-pitch-ts), versão de pacote **1.0.1**, licença Apache 2.0. Os assets locais foram obtidos do commit `2d498f82b61c71898edf0e8dd661b99076676c8b`; fontes individuais e licença estão em [PROVENANCE.json](../public/models/basic-pitch/PROVENANCE.json). O [README oficial](https://github.com/spotify/basic-pitch-ts/blob/2d498f82b61c71898edf0e8dd661b99076676c8b/README.md) documenta `BasicPitch`, `evaluateModel`, `outputToNotesPoly` e `noteFramesToTime`.

| Asset carregado | SHA-256 verificado nos bytes usados |
| --- | --- |
| `model.json` | `1ed1aaee3409ec1dc098c8b01f430c0911f6fe9412e7af8086750f9e8f302f68` |
| `group1-shard1of1.bin` | `b142a95737a52e1e412d5f92e73d8bb80dfe8d04941acc0702f11f4524fb377c` |

## Amostra fixada antes da execução

- `vocadito_1 + vocadito_21`.
- `vocadito_2 + vocadito_22`.
- `vocadito_3 + vocadito_23`.

Cada fonte usa seus **primeiros 8 segundos**, começando em 0, sem deslocar a voz ou a anotação. FFmpeg apenas decodifica cada fonte, preservando taxa e canais originais. O `resampleMono` da aplicação converte para mono/22.050 Hz; mistura `0.5*a + 0.5*b`. Não há normalização individual por pico, mudança de altura, time stretch ou alinhamento musical. São 24 segundos de mistura provenientes de 48 segundos de fontes reais, com simultaneidade artificial e frases que não foram gravadas como harmonias entre si.

Os pares foram especificados para este teste; não são uma seleção aleatória representativa de duos/coros. Um smoke inicial exploratório confirmou o custo de inferência, mas usava resampler FFmpeg e thresholds 0,25/0,25, diferentes do app. Esses resultados exploratórios não são o baseline de produção e ficaram somente no cache privado. O presente relatório e JSON foram regenerados com resampler e parâmetros de produção. Como duas condições mudaram, não atribuímos diferenças a uma delas isoladamente. Os três pares de 8 s foram concluídos, sem reduzir a amostra por desempenho.

## Implementação e ambiente

- Avaliador: [scripts/benchmark-polyphonic.mjs](../scripts/benchmark-polyphonic.mjs).
- SHA-256 do avaliador: `3eddbc7da0b2bcee1e3cd160fc3247c2061a36d0087a5b651bd098e968136fb1`.
- Node v22.22.3; TensorFlow.js 3.21.0; backend **CPU JavaScript**, darwin/arm64; FFmpeg 9.0.2. Nenhum backend nativo foi instalado para o teste.
- `tf.loadGraphModel(tf.io.fromMemory(...))` lê JSON e weights diretamente do filesystem; `new BasicPitch(Promise.resolve(graphModel))` evita fetch remoto.
- Entrada: `Float32Array` mono a 22.050 Hz. `resampleMono` é importado diretamente de `src/audio/pitch.ts`; FFmpeg não substitui esse resampler. Modelo e pesos são os mesmos assets disponíveis para a implementação web.
- Pós-processamento: `noteFramesToTime(outputToNotesPoly(frames, onsets, 0.5, 0.3, 5))`. Threshold de onset **0,5**, frame **0,3** e mínimo de **5 frames**, exatamente como `src/audio/polyphonic.worker.ts`, sem ajuste por par. O avaliador lê esse arquivo e falha se os três parâmetros divergem; também exige Basic Pitch 1.0.1 e TensorFlow.js 3.21.0.
- Hash de `src/audio/polyphonic.worker.ts`: `d85a2f987fadbbf85178b67689f7dc4c3377fe98f241c511e6ee609b470e28db`.
- Hash de `src/audio/pitch.ts`, que contém o resampler: `56e171ef2ef28ded0ee959ed8dc943e67f4ff01b211c67b0caa4e03433726541`.
- A avaliação executa os mesmos parâmetros e função de resampling, com escopo TensorFlow ao redor da inferência, mas não chama a classe de worker/browser nem o merge de chunks da aplicação. Não mede carregar assets, UI ou transferir dados. Inferência e pós-processamento são medidos no Node; integração web e chunks longos devem ser validados separadamente.

## Métricas

Uma grade comum de **10 ms** cobre `[0, 8)` s de cada mistura, totalizando 2400 frames. A referência contém os F0 contínuos publicados das duas fontes, obtidos pelo timestamp de anotação mais próximo. Não há deslocamento temporal otimizado nem quantização da referência.

A predição contém as notas ativas no instante da grade, convertidas do **centro MIDI inteiro** para Hz. Pitches MIDI repetidos entre notas sobrepostas são deduplicados. Pitch bends e os contornos contínuos do modelo não entram neste protocolo; por isso esta é uma **aproximação de multi-F0 a partir de eventos de nota**, não uma avaliação direta de todos os contornos de F0 do modelo.

Matching de máxima cardinalidade **1:1** em cada frame com `abs(1200*log2(pred/ref)) <= 50`. Uma predição não pode acertar duas referências. Oitava diferente é erro. TP são matches; FP são pitches previstos sem match; FN são pitches de referência sem match. Precisão `TP/(TP+FP)`, recall `TP/(TP+FN)` e F1 harmônica; os totais abaixo agregam contagens, não a média simples dos percentuais dos pares.

A referência preserva as duas fontes mesmo quando chegam a uníssono. Uma predição única pode então contar no máximo um match. Para deixar esse limite explícito, também reportamos separadamente frames onde ambas estão vozeadas e afastadas por mais de 100 cents. Esse subconjunto é derivado da referência, não selecionado pelo desempenho.

O script inclui sanity checks do matching: oitava não acerta, uma predição não conta duas vezes, e o algoritmo encontra a máxima cardinalidade quando precisa remapear matches.

## Resultados desta amostra

| Par, primeiros 8 s | Inferência + pós CPU s | Notas previstas | Precisão ±50c | Recall ±50c | F1 |
| --- | ---: | ---: | ---: | ---: | ---: |
| vocadito_1+vocadito_21 | 6.53 | 32 | 84.56% | 42.51% | 56.58% |
| vocadito_2+vocadito_22 | 6.47 | 47 | 75.87% | 71.44% | 73.59% |
| vocadito_3+vocadito_23 | 6.41 | 43 | 83.79% | 63.14% | 72.01% |

| Agregação | Frames | TP | FP | FN | Precisão | Recall | F1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Todos os frames das 3 misturas | 2400 | 1838 | 448 | 1291 | 80.40% | 58.74% | 67.89% |
| Somente 2 vozes separadas >100c | 964 | 1123 | 242 | 805 | 82.27% | 58.25% | 68.21% |

No total, 3129 pitches de referência e 2286 previstos foram avaliados. Houve 202 frames com duas referências separadas por até 100 cents. O JSON [BENCHMARK_POLYPHONIC_RESULTS.json](BENCHMARK_POLYPHONIC_RESULTS.json) conserva contagens por par, notas previstas, hashes das fontes, hashes das misturas em memória e tempos.

As 3 inferências e pós-processamentos levaram 19.40 s em CPU para 24 s de áudio; execução total do avaliador após carregar o modelo: 20.00 s. É uma observação desta máquina, não uma garantia de velocidade ou consumo de memória no navegador.

## Reproduzir

Requer Node ≥22.12 com `--experimental-strip-types`, FFmpeg/ffprobe e as versões de dependências fixadas no checkout. Após preparar o cache Vocadito conforme [BENCHMARK.md](BENCHMARK.md):

```sh
node --experimental-strip-types scripts/benchmark-polyphonic.mjs --dataset "$HOME/.cache/vocal-practice-benchmark/corpus" --pairs 1 --seconds 8 --output "$HOME/.cache/vocal-practice-benchmark/polyphonic-smoke.json"
node --experimental-strip-types scripts/benchmark-polyphonic.mjs --dataset "$HOME/.cache/vocal-practice-benchmark/corpus" --pairs 3 --seconds 8 --output "$HOME/.cache/vocal-practice-benchmark/polyphonic-results.json"
```

`--model` pode apontar para outro diretório local com `model.json`, seus shards e `PROVENANCE.json`; o JSON registra os hashes reais dos assets. `--pairs 1` é explicitamente apenas um smoke test e não substitui o agregado dos três pares.

## Limites da conclusão

Não mede coros gravados juntos, reverberação compartilhada, harmonia musical coordenada, seis ou mais vozes, separação de cantores, identificação de qual voz seguir, pitch do microfone, pontuação ao vivo ou qualidade perceptual. O centro MIDI ignora vibrato e desvios expressivos; limites de notas e timestamps influenciam a métrica. Os números não demonstram transcrição confiável de qualquer backing vocal polifônico. Esta amostra serve como evidência inicial de comportamento e como regressão reproduzível, complementando o benchmark monofônico.
