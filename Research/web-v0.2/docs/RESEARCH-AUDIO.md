# Pesquisa de pitch vocal e múltiplas vozes

Consulta: 2026-11-16. Escopo: decisões de engenharia para a aplicação web; esta revisão de fontes não demonstra a precisão da implementação. Os benchmarks executados devem ser registrados separadamente com versão, parâmetros, dataset e resultados.

## Decisão para a próxima versão

Manter o microfone como entrada monofônica e separar dois caminhos para o arquivo alvo:

1. **Uma voz isolada:** detector de F0 monofônico, curva contínua e notas derivadas. A qualidade deve ser medida com canto real anotado, além de sinais sintéticos.
2. **Várias vozes simultâneas:** análise de múltiplos pitches para produzir **candidatos**. Mostrar as notas simultâneas e permitir escolher/revisar uma linha de estudo. Atribuir soprano/alto/tenor/baixo exige informação e validação adicionais; frequência mais alta, volume maior e continuidade não provam identidade de naipe.

Basic Pitch é uma opção prática para experimentar o segundo caminho localmente no navegador. Não equivale a separar stems e não oferece garantia de recuperar todas as linhas de um backing vocal. Uma linha gerada deve começar sem pontuação habilitada; seleção e revisão explícitas habilitam o alvo utilizado pelo scorer. Ao trocar modelo, thresholds, região ou seleção, invalidar a revisão e a tentativa. Não converter automaticamente um resultado polifônico em uma única referência com `argmax`.

## O que cada família resolve

| Método | Saída e contribuição | Uso e limite para este projeto |
| --- | --- | --- |
| YIN, de Cheveigné e Kawahara, 2002 | Estimativa de uma periodicidade/F0, com função de diferença, normalização e refinamento. | Baseline simples para microfone e uma voz isolada. A hipótese monofônica não resolve notas simultâneas. [Paper](https://www.ee.columbia.edu/~dpwe/e6820/papers/deChevK02-yin.pdf). |
| pYIN, Mauch e Dixon, 2014 | Candidatos probabilísticos e HMM/Viterbi para escolher uma trajetória monofônica. | Melhor baseline offline para erros de oitava e continuidade. Vários candidatos representam interpretações alternativas de **uma** voz, não vozes simultâneas recuperadas. [Paper dos autores](https://webspace.eecs.qmul.ac.uk/s.e.dixon/pub/2014/MauchDixon-PYIN-ICASSP2014.pdf). |
| CREPE, Kim et al., 2018 | CNN sobre waveform para pitch monofônico; implementação oferece confiança e tamanhos de modelo. | Comparador neural para voz isolada. O demo browser usa uma versão reduzida, cuja qualidade não deve ser confundida com os resultados do modelo completo. [Paper](https://arxiv.org/abs/1802.06182), [implementação](https://github.com/marl/crepe), [demo oficial](https://marl.github.io/crepe/). |
| RMVPE, Wei et al., 2023 | Extração robusta de pitch vocal na presença de acompanhamento. | Relevante para uma melodia vocal predominante em mix. “Polyphonic music” no título não significa recuperar simultaneamente todos os cantores de um coro. Não adotar como solução multi-voz sem experimento específico. [Paper](https://arxiv.org/abs/2306.15412). |
| Basic Pitch, Bittner et al., 2022 | Rede com saídas de onsets, notas e multipitch; generalista, inclui voz. | Boa opção disponível para gerar candidatos polifônicos. O paper não estabelece recuperação confiável de identidade SATB. Sua avaliação vocal é majoritariamente de sinais monofônicos; não transportar a precisão desses testes para coro. [Paper](https://arxiv.org/abs/2203.09893), [código TS oficial](https://github.com/spotify/basic-pitch-ts). |
| Deep Salience, Bittner et al., 2017 | Rede convolucional aprende uma representação de saliência de F0 para melodia e multi-F0. | Referência para distinguir mapa de atividade de pitch, extração de trajetória e identidade de fonte. Estes são estágios diferentes. [Paper dos autores](https://brianmcfee.net/papers/ismir2017_salience.pdf). |
| Cuesta, McFee e Gómez, 2020 | CNNs específicas para multi-F0 em ensembles vocais a cappella, com magnitude HCQT e diferenças de fase. | Baseline de domínio mais pertinente para harmonia vocal que um detector monofônico. Usa misturas de stems, cenários com reverb e referências de qualidade variável. [Paper](https://arxiv.org/abs/2009.04172). |
| Koh e Dong, outubro de 2026 | Reavalia STFT linear contra HCQT para multi-pitch de ensembles vocais. | Resultado recente favorece STFT no protocolo dos autores, com menor custo de extração. É direção de pesquisa, não prova de que Basic Pitch é o melhor modelo nem um pacote browser pronto. [Preprint v2](https://arxiv.org/html/2610.03656v2). |

O trabalho de 2026 reconhece referências majoritariamente automáticas, frequentemente pYIN, e predomínio de misturas de stems gravados separadamente. Resultados são menos consistentes nos conjuntos com cantores gravados juntos. A conclusão útil é comparar representações e testar conjuntos independentes, sem assumir que maior resolução espectral resolve por si só o problema de várias vozes. [Limitações e resultados](https://arxiv.org/html/2610.03656v2#S4.SS3).

## Contrato prático de Basic Pitch no browser

Context7 foi consultado: resolveu `/spotify/basic-pitch`, mas retornou principalmente documentação Python. As assinaturas TypeScript abaixo foram conferidas no código oficial TS, sem assumir equivalência entre as APIs.

```ts
const model = new BasicPitch('/models/basic-pitch/model.json');
await model.evaluateModel(mono22050, (f, o, c) => {
  frames.push(...f);
  onsets.push(...o);
  contours.push(...c);
}, progress);
```

A entrada pode ser `Float32Array` mono já reamostrado para 22.050 Hz. A classe carrega um GraphModel por URL. O processamento usa janelas sobrepostas de aproximadamente dois segundos e entrega matrizes por callback. Seu código não faz descarte explícito de todos os tensores criados. Para a experiência inicial, isolar um job em Worker descartável evita manter essas alocações entre análises. [Fonte oficial de inferência](https://raw.githubusercontent.com/spotify/basic-pitch-ts/main/src/inference.ts).

Decodificar com `outputToNotesPoly(frames, onsets, onsetThreshold, frameThreshold, minNoteLen)`, opcionalmente `addPitchBendsToNoteEvents`, depois `noteFramesToTime`. Os eventos têm `startTimeSeconds`, `durationSeconds`, `pitchMidi`, `amplitude` e `pitchBends` opcional. `minNoteLen` é em frames nessa API, não em milissegundos. Usar a conversão temporal da biblioteca, em vez de inventar uma relação linear que ignore sua correção de janelas. O pitch nominal de um evento é discreto; os contours têm três bins por semitom. [Fonte oficial de decodificação](https://raw.githubusercontent.com/spotify/basic-pitch-ts/main/src/toMidi.ts).

Os assets precisam permanecer juntos: `model/model.json` e o shard relativo `group1-shard1of1.bin`. Na consulta direta ao repositório, mediram respectivamente 174.537 e 742.392 bytes, 916.929 bytes no total. O manifest enumera 185.598 valores de pesos/tensores; isso **não é** uma contagem verificada de parâmetros treináveis. O artefato distribuído e o modelo original do paper não devem ser descritos como idênticos. Preservar licença e atribuição Apache-2.0 ao distribuir os assets. [Manifest](https://raw.githubusercontent.com/spotify/basic-pitch-ts/main/model/model.json), [licença](https://github.com/spotify/basic-pitch-ts/blob/main/LICENSE).

## Limites operacionais propostos

Estes limites são escolhas de engenharia para a experiência inicial, não números de desempenho demonstrados por papers:

- A proposta inicial de limitar uma análise neural a 60 segundos foi substituída por processamento em blocos: a implementação aceita o limite existente de 15 minutos por arquivo, mas cada Worker recebe somente um core de **20 segundos**, com até 0,5 segundo de contexto de cada lado.
- Worker exclusivo, fila de um job, cancelamento por `terminate`, watchdog de 120 segundos e encerramento do Worker também ao concluir. Preservar a sessão anterior quando houver falha ou cancelamento.
- Mostrar carregamento do modelo, progresso, tempo decorrido e motivo de falha. Revalidar esta política depois de medir desktop e dispositivos com menos memória.
- Evitar que matrizes brutas atravessem o Worker para React. Enviar candidatos compactos e metadados. Não manter cópias desnecessárias de waveform, PCM e outputs.
- O código recorta os eventos ao core de cada bloco e só une a mesma nota na fronteira quando os dois blocos predizem que ela atravessa esse limite. Casos de notas sustentadas, reataques e simultaneidade têm testes. Essa política evita duplicação pelo contexto; ainda precisa de validação auditiva e quantitativa com vozes reais.

Uma estimativa inferior para as três matrizes de saída é `86 frames/s × (88+88+264) bins × 4 bytes`, cerca de 9,1 MB por minuto se fossem Float32 compactos. A API usa `number[][]`; números JS, arrays, tensores, PCM, kernels e clones aumentam o consumo. Esse cálculo não é peak memory medido. O tamanho pequeno do arquivo do modelo não demonstra baixo pico de memória; o próprio paper diferencia tamanho dos pesos e tamanho dos feature maps. [Discussão de memória no paper](https://arxiv.org/pdf/2203.09893).

Registrar backend TFJS, versão exata do pacote, hardware, navegador, tempo frio/quente, fator de tempo real, p50/p95, contagem de tensores e memória quando mensurável. Um Worker melhora responsividade da interface; ele não torna a inferência automaticamente rápida.

## Dados para validar

| Conjunto | Por que usar | Restrições e cuidado com a referência |
| --- | --- | --- |
| vocadito | 40 trechos reais de canto solo, sete idiomas, dispositivos e experiência variados; F0 e duas versões de notas anotadas por músicos. É um primeiro teste pequeno e pertinente para a trajetória monofônica. | Separar desenvolvimento/aceite e reportar resultados por trecho. Metadados da versão consultada indicam **CC BY 4.0**. [Registro primário](https://zenodo.org/records/5578807), [metadados](https://zenodo.org/api/records/5578807). |
| Choral Singing Dataset, CSD v5 | 16 cantores, três peças, tracks individuais, F0, anotações de notas e MIDI semi-sincronizado. Permite testar solo, uníssono e mix coral com referências conhecidas. | Não confundir com Children's Song Dataset. Metadados oficiais consultados indicam **CC BY-NC 4.0**; atribuir e manter dados fora dos assets do produto por padrão. [Registro](https://zenodo.org/records/2649950), [metadados](https://zenodo.org/api/records/2649950). |
| Dagstuhl ChoirSet, DCS v1.2.3 | Ensemble amador gravado em conjunto, settings de coro e quartetos, microfones próximos e mix. Importante para bleed e interações reais. | F0 disponibilizado é extraído automaticamente; não tratá-lo todo como verdade humana independente. Metadados indicam **CC BY-NC-SA 4.0**. [Registro](https://zenodo.org/records/4618287), [metadados](https://zenodo.org/api/records/4618287), [documentação](https://www.audiolabs-erlangen.de/resources/MIR/2020-DagstuhlChoirSet). |
| Cantoría | Quarteto profissional, 11 peças, stems e mixes SATB com/sem órgão, trajetórias pYIN e CREPE. Amplia timbres e textura. | Tracks foram gravadas separadamente; referência automática não valida independentemente o detector que a produziu. Metadados da versão consultada indicam **CC BY 4.0**. [Registro](https://zenodo.org/records/5851070), [metadados](https://zenodo.org/api/records/5851070). |

As licenças acima vieram dos metadados dos registros primários, não de agregadores: alguns resumos externos descrevem incorretamente CSD e DCS como CC BY sem restrição NC. Um benchmark pode manter apenas manifesto, checksums, scripts e resultados no repositório, com o corpus obtido separadamente.

## Protocolo de avaliação

### Monofonia

Fixar um timebase comum e preservar silêncio. Calcular RPA com tolerância de 50 cents, RCA, voicing recall, voicing false alarm e overall accuracy. RPA avalia afinação absoluta nos frames de referência com voz; RCA remove a diferença de oitava. Portanto RCA alta com RPA baixa aponta erros de oitava, não sucesso suficiente para estudo vocal. Recall e falso alarme revelam o compromisso entre perder notas e inventar voz durante silêncio. [Definições oficiais mir_eval](https://mir-eval.readthedocs.io/latest/api/melody.html).

Acrescentar MAE/mediana/p95 de erro em cents em correspondências válidas, taxa de erro de oitava, resultados por tessitura e latência temporal. Não melhorar métricas deslocando cada gravação por um offset livre escolhido a partir da resposta: separar atraso conhecido do pipeline e alinhamento permitido pelo protocolo. Comparar YIN atual, pYIN e uma opção neural com exatamente os mesmos recortes.

### Polifonia

Para cada frame, comparar **conjuntos** de F0 com matching um a um, tolerância inicial de 50 cents; reportar precision, recall e F1, além de miss, false alarm e substitution. Repetir em 20 cents quando a referência suportar essa precisão. Um único pitch estimado não pode acertar dois cantores. Métricas de chroma devem ficar separadas das absolutas. Uníssono exige política explícita sobre multiplicidade e dispersão; pitch set não mede identidade de cantor. [Definições oficiais mir_eval](https://mir-eval.readthedocs.io/latest/api/multipitch.html).

Acrescentar continuidade da linha escolhida, erros de troca de voz, notas espúrias, onset/offset e custo de revisão humana. Identidade SATB só pode ser avaliada quando a referência identifica a parte e o modelo realmente produz essa atribuição.

### Matriz mínima de cenários

- Tons isolados, harmônicos fortes, vibrato, portamento, transições rápidas e consoantes sem voz.
- Canto solo real em registros baixos/altos e diferentes níveis de ganho; ruído, clipping, reverberação e compressão.
- Duas a quatro vozes, terças/quintas/oitavas, cruzamento de linhas, uníssono, entradas desencontradas e uma voz muito mais baixa que as demais.
- Mesmo material em stems, mixes controlados e gravação conjunta. Splits por música/cantor/sessão, para que remixes da mesma gravação não contaminem desenvolvimento e teste.
- Playback real: silêncio do microfone durante alvo conta como ausência; silêncio do alvo não aumenta o score; cantar outra oitava continua sendo erro absoluto. Pitch candidato não revisado mantém o score bloqueado.
- Lifecycle: cancelar, trocar arquivo, buscar durante análise, desconectar interface, permissão recusada, timeout e repetição de jobs não deixam streams, workers ou alvos antigos ativos.

Nenhum threshold universal deve ser escolhido só para fazer um exemplo parecer correto. Publicar parâmetros e resultados por cenário; manter os casos de falha. Para a v0 seguinte, a afirmação defensável é “candidatos polifônicos experimentais com revisão”, até haver evidência de recuperação de linhas vocais reais.

## Implementação decorrente desta revisão

Foram adicionados um Worker polifônico descartável por bloco, pipeline de trim/merge, assets locais com SHA-256 e commit de origem, cancelamento de análise e invalidação de decodes pendentes. O modo polifônico retorna apenas candidatos de nota (`pitch=[]`); criar uma referência monofônica pontuável continua sendo responsabilidade explícita da seleção e revisão na interface. As dependências foram fixadas em `@spotify/basic-pitch@1.0.1` e `@tensorflow/tfjs@3.21.0`, compatível com a major 3 declarada pela biblioteca. A inferência usa CPU para funcionar no Worker sem depender de contexto WebGL.

Os testes determinísticos cobrem simultaneidade, fronteiras de blocos, reataques, cancelamento, timeout, descarte do Worker e cancelamento que chega durante decode. Eles comprovam contrato e gerenciamento de recursos, não precisão de transcrição coral. Resultados medidos em corpus e no navegador devem constar nos documentos de validação/benchmark, sem substituir estas limitações.
