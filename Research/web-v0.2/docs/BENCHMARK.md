# Benchmark de F0 com canto real — Vocadito

Avaliação executada em 2026-11-16. **Todos os 40 clips do corpus**, sem amostragem: 817.13 s de áudio. O detector da v0 obteve **RPA ±50 cents de 84.56%** neste protocolo. Isso revela erros relevantes em canto real; os testes de sinais sintéticos não estabelecem precisão de transcrição vocal.

## Corpus, licença e atribuição

[Vocadito, registro Zenodo 5578807, versão v3](https://zenodo.org/records/5578807), de Rachel M. Bittner, Katherine Pasalo, Juan José Bosch, Gabriel Meseguer-Brocal e David Rubinstein. Contém excertos solo monofônicos e F0 anotado por músicos. A [metadata oficial do registro](https://zenodo.org/api/records/5578807) declara **CC BY 4.0**. O [artigo técnico, arXiv:2110.05580v2](https://arxiv.org/abs/2110.05580v2), descreve o corpus e distingue F0 contínuo de notas segmentadas.

O ZIP e áudios foram mantidos exclusivamente em cache privado fora do repositório. O repositório contém o avaliador e resultados numéricos derivados, com esta atribuição. Nenhum áudio, CSV de anotação completo ou transcrição de letra foi copiado para ele.

- ZIP: `vocadito.zip`, 58.492.257 bytes.
- MD5 publicado e verificado: `dea40fd18f14d899643c4ba221b33a46`.
- SHA-256 do ZIP: `e0d6b99d3f9c594afe5ae5c4d7bdacebe569e53b809e90b89d1c771c4f9990e3`.
- SHA-256 de cada WAV e CSV: [BENCHMARK_RESULTS.json](BENCHMARK_RESULTS.json).

## Implementação medida

Executa diretamente `resampleMono` e `analyzePitch` de `src/audio/pitch.ts`, o mesmo caminho usado pelo worker de análise da voz alvo. Nenhuma implementação alternativa de YIN foi usada e nenhum parâmetro do detector foi alterado para o benchmark.

- Commit base: `89e5ce9d07dde7ad707179187f9ee46d36303f34`.
- SHA-256 de `src/audio/pitch.ts`: `56e171ef2ef28ded0ee959ed8dc943e67f4ff01b211c67b0caa4e03433726541`.
- SHA-256 do avaliador: `d491939b9c33b4b7b71eac754ede70ee425ddd63209b26936c2840b78568b0df`.
- Node v22.22.3, darwin/arm64; FFmpeg 9.0.2.
- FFmpeg decodifica para float32 na taxa e quantidade de canais originais; nosso `resampleMono` converte para 16 kHz. Assim, FFmpeg não substitui o resampler da aplicação.
- Janela 1.024 amostras/64 ms; hop 25 ms; faixa 65–1.200 Hz; limiar de confiança 0,82; gate RMS 0,006. Sem normalização de volume extra.
- Tempo observado da execução completa: 6.71 s, incluindo decodificação e avaliação. É uma observação desta máquina, não promessa de desempenho no navegador.

## Protocolo

A avaliação usa todos os timestamps da anotação F0 dentro da duração de cada WAV. Para cada timestamp, escolhe o frame previsto temporalmente mais próximo; empate escolhe o anterior. Não há busca de deslocamento temporal, tolerância extra de timing, interpolação da referência, quantização da referência em semitons ou correção de oitava.

Referência vozeada: F0 > 0, conforme o README do corpus. Estimativa vozeada: frequência > 0 e confiança ≥0,82. A grade de referência contém 140,782 frames, sendo 92,966 vozeados e 47,816 não vozeados. Frames próximos não são observações estatisticamente independentes.

- **RPA ±50c**: `abs(1200 * log2(estimativa / referência)) <= 50`, dividido por todos os frames de referência vozeados. Frames sem detecção contam como erro. Uma oitava errada não é acerto.
- **RCA ±50c**: mesma distância, reduzida módulo 1.200 cents; aceita oitavas diferentes. Usa o mesmo denominador da RPA.
- **Voicing recall**: referência vozeada detectada como vozeada / referência vozeada.
- **Voicing false alarm**: referência não vozeada detectada como vozeada / referência não vozeada.
- **Erro de oitava**: RCA correta e RPA incorreta. Contagem separada por direção, sem esconder esses erros na RPA.
- **Micro** soma contagens do corpus. **Macro** calcula a média simples das métricas dos 40 clips.

## Resultados do corpus inteiro

| Métrica | Micro, todos os frames | Macro, média dos 40 clips |
| --- | ---: | ---: |
| RPA ±50 cents | 84.56% | 84.08% |
| RCA ±50 cents | 85.69% | 84.97% |
| Voicing recall | 91.57% | 91.13% |
| Voicing false alarm | 10.42% | 11.60% |

Foram 78,610 acertos de pitch em 92,966 frames vozeados; 7,841 frames vozeados ficaram sem detecção e 4,982 frames de silêncio anotado receberam uma frequência. Houve 1,055 erros de oitava: 713 acima e 342 abaixo, equivalentes a 1.13% dos frames de referência vozeados. Outros 5,460 frames detectados erraram mais de 50 cents sem ser um acerto de chroma. Nenhum F0 vozeado de referência ficou fora da faixa 65–1.200 Hz.

## Onde falha

A RPA varia de 69.07% a 97.00%. Os piores clips são `vocadito_30` (69,07%), `vocadito_21` (69,21%) e `vocadito_26` (69,28%). Não atribuímos a causa a uma técnica vocal ou condição acústica sem inspeção adicional.

Exemplos diretamente comparados com a anotação:

- `vocadito_30`, 0,86494 s: referência 175,109 Hz, estimativa 347,329 Hz, +1.185,66 cents — quase uma oitava acima.
- `vocadito_34`, 1,28871 s: referência 265,669 Hz, estimativa 132,351 Hz, −1.206,31 cents — quase uma oitava abaixo.
- `vocadito_26`, 1,01007 s: referência 109,494 Hz, estimativa 103,204 Hz, −102,42 cents — erro superior a um semitom, sem erro de oitava.

O gate de RMS estava abaixo do limiar em 750 dos 7841 frames vozeados perdidos. Isso é associação observada, não prova de que mudar o gate corrigiria os misses restantes. O false alarm chega a 32,22% em `vocadito_10`; o denominador é o silêncio anotado daquele clip.

## Por clip, sem seleção

| Clip | Duração s | RPA ±50c | RCA ±50c | Voicing recall | False alarm | Erros de oitava |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vocadito_1 | 33.2 | 88.33% | 88.41% | 92.17% | 2.79% | 3 |
| vocadito_2 | 35.2 | 85.82% | 94.40% | 97.05% | 1.60% | 392 |
| vocadito_3 | 21.7 | 85.40% | 85.40% | 89.17% | 2.79% | 0 |
| vocadito_4 | 18.5 | 79.90% | 79.90% | 85.95% | 3.49% | 0 |
| vocadito_5 | 23.2 | 86.39% | 86.39% | 92.64% | 22.11% | 0 |
| vocadito_6 | 23.8 | 83.19% | 83.51% | 95.84% | 6.81% | 9 |
| vocadito_7 | 19.5 | 88.68% | 88.68% | 92.99% | 10.38% | 0 |
| vocadito_8 | 17.0 | 85.05% | 85.31% | 88.67% | 2.16% | 4 |
| vocadito_9 | 20.4 | 83.53% | 83.78% | 91.84% | 8.55% | 5 |
| vocadito_10 | 9.1 | 77.97% | 77.97% | 84.92% | 32.22% | 0 |
| vocadito_11 | 13.0 | 87.81% | 87.81% | 93.38% | 15.66% | 0 |
| vocadito_12 | 15.2 | 96.08% | 96.29% | 98.15% | 15.67% | 4 |
| vocadito_13 | 22.4 | 79.34% | 79.34% | 87.77% | 7.62% | 0 |
| vocadito_14 | 12.2 | 85.36% | 85.36% | 93.28% | 13.80% | 0 |
| vocadito_15 | 20.3 | 81.76% | 83.45% | 88.29% | 7.97% | 39 |
| vocadito_16 | 17.3 | 87.96% | 87.96% | 94.35% | 10.18% | 0 |
| vocadito_17 | 23.6 | 81.08% | 82.38% | 88.86% | 0.40% | 43 |
| vocadito_18 | 29.1 | 87.77% | 87.77% | 95.40% | 4.75% | 0 |
| vocadito_19 | 30.5 | 82.34% | 82.49% | 89.86% | 12.99% | 5 |
| vocadito_20 | 8.7 | 82.84% | 83.26% | 93.60% | 9.93% | 4 |
| vocadito_21 | 22.0 | 69.21% | 71.77% | 85.13% | 21.39% | 76 |
| vocadito_22 | 18.3 | 85.50% | 85.50% | 89.61% | 5.45% | 0 |
| vocadito_23 | 18.0 | 76.96% | 78.94% | 84.23% | 3.50% | 39 |
| vocadito_24 | 14.8 | 87.58% | 89.89% | 93.67% | 28.75% | 38 |
| vocadito_25 | 14.7 | 90.26% | 90.61% | 96.48% | 18.61% | 6 |
| vocadito_26 | 12.9 | 69.28% | 69.28% | 85.19% | 20.27% | 0 |
| vocadito_27 | 14.0 | 88.38% | 88.38% | 93.06% | 10.73% | 0 |
| vocadito_28 | 12.9 | 83.36% | 83.36% | 91.81% | 7.06% | 0 |
| vocadito_29 | 14.9 | 91.81% | 91.81% | 94.01% | 19.57% | 0 |
| vocadito_30 | 13.0 | 69.07% | 73.24% | 83.31% | 20.08% | 51 |
| vocadito_31 | 14.0 | 82.83% | 82.83% | 88.53% | 14.89% | 0 |
| vocadito_32 | 25.0 | 92.05% | 92.05% | 95.74% | 21.10% | 0 |
| vocadito_33 | 30.0 | 77.32% | 79.57% | 88.67% | 12.98% | 76 |
| vocadito_34 | 24.9 | 71.70% | 78.90% | 84.32% | 9.35% | 226 |
| vocadito_35 | 13.8 | 83.92% | 83.92% | 88.81% | 6.35% | 0 |
| vocadito_36 | 23.3 | 84.15% | 84.15% | 87.29% | 11.44% | 0 |
| vocadito_37 | 38.7 | 97.00% | 97.24% | 98.71% | 6.96% | 12 |
| vocadito_38 | 27.8 | 85.91% | 85.91% | 90.74% | 9.00% | 0 |
| vocadito_39 | 31.7 | 92.46% | 92.48% | 97.04% | 12.67% | 1 |
| vocadito_40 | 18.6 | 87.95% | 89.03% | 94.71% | 12.08% | 22 |

## Reproduzir

Requer Node ≥22.12, FFmpeg/ffprobe, curl e unzip. Execute a partir do checkout. O diretório de cache abaixo fica fora do Git.

```sh
benchmark_cache="$HOME/.cache/vocal-practice-benchmark"
mkdir -p "$benchmark_cache/corpus"
curl -fL --retry 2 'https://zenodo.org/records/5578807/files/vocadito.zip?download=1' -o "$benchmark_cache/vocadito.zip"
unzip -q -o "$benchmark_cache/vocadito.zip" -d "$benchmark_cache/corpus"
node --experimental-strip-types scripts/benchmark-pitch.ts --dataset "$benchmark_cache/corpus" --archive "$benchmark_cache/vocadito.zip" --output "$benchmark_cache/results.json"
```

`--archive` verifica o MD5 oficial antes de avaliar e grava os hashes do arquivo. A opção `--limit N` permite um smoke test dos primeiros N clips em ordem numérica; esse resultado é rotulado como subconjunto e não substitui os números do corpus inteiro acima. O JSON contém contagens, hashes por arquivo, métricas por clip e exemplos de erro.

## Limites da conclusão

Este é um benchmark de F0 monofônico offline em 40 excertos de um corpus. Não mede o caminho de captura ao vivo, ruído de interface, latência física, bleed da backing track, gravações polifônicas, precisão da segmentação de notas, intervalos ou pontuação da sessão. Em particular, notas MIDI arredondadas não substituem uma avaliação própria de transcrição de notas. Não é uma validação do canto de Matheus. Os testes unitários existentes cobrem contratos e sinais controlados; não eliminam os erros reais medidos aqui.
