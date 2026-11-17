# Validação da v0

Executada em 2026-11-16. Não equivale a medição de precisão com canto real.

## Automatizada

38 testes em Vitest: detector com sinais sintéticos de diversas frequências, harmônicos, ruído/silêncio e segmentação; relógio, seek, faixas de duração diferente e loop; geração de captura/compensação temporal; lifecycle do mic (stream, canal, troca, worklet e cancelamento tardio); pontuação com silêncio, oitava errada, tolerância e denominador independente dos callbacks; continuidade do score com hop de 48 kHz.

TypeScript e build de produção via Vite passaram. CI executa instalação reproduzível, testes e build.

## Navegador

Testes feitos no navegador Chromium integrado:

- Demonstração: abertura, reprodução, pausa com Espaço, Enter, seek por teclado, zoom e mute individual.
- Importação real de dois WAVs sintéticos de quatro segundos; referência de 440 Hz exibida como A4, 160 frames analisados em Worker.
- Conexão com uma interface Scarlett 2i2 USB; dois canais oferecidos e seleção entre eles; alteração de janela, compensação e leitura de diagnósticos (44,1 kHz). Nenhum teste de canto ou medição física de round trip foi feito.
- Arquivo WAV inválido foi rejeitado com erro legível e a sessão anterior preservada.
- Retorno começa desligado. Entrada sem sinal durante reprodução produziu 0% de acerto e cobertura; tempo de alvo avaliado avançou.
- Layout de desktop e viewport estreito; sem overflow horizontal na bancada desktop. Gráficos respondem a resize.

## Pendências de uso real

Cantar sobre stems reais, validar a estabilidade do detector com timbre/ruído/reverberação do ambiente, ajustar compensação e conforto do retorno. Testar navegadores além de Chromium e interfaces além da entrada disponível. Não afirmar que a latência informada pelo browser é uma medição ponta a ponta.

## Atualização v0.2 — pesquisa, backing opcional e candidatos polifônicos

59 testes passaram, mais build TypeScript/Vite. Novos casos cobrem manutenção de notas simultâneas, contexto/bordas/reataque em chunks, timeout, cancelamento tardio e durante decode, remoção de faixa, bloqueio de score antes de revisão, filtro da faixa do microfone, escolha de registro, margem de captura e resume sem preencher a cauda excluída.

No navegador, foi importado um alvo mono de 4 s sem base e exibido A4. O mixer mostrou somente o alvo e o loop foi limitado à duração. Um acorde sintético polifônico de 8 s executou o modelo local no Worker e gerou cinco candidatas, incluindo um harmônico alto espúrio (D6). Ao restringir C4–G4, a seleção mais aguda exibiu G4. Isso é um caso de falha preservado, não uma prova de transcrição perfeita do acorde. A pontuação começou bloqueada, foi liberada por confirmação e voltou ao bloqueio após trocar o critério. Cancelar análise de um arquivo maior preservou o alvo anterior de 8 s. Um arquivo de 24 s completou a análise em dois blocos e retornou 18 candidatas, verificando a integração dos Workers além de um único bloco.

O baseline mono foi executado em todos os 40 clips Vocadito: RPA50c84,56%, RCA85,69%, voicingrecall91,57%, falsealarm10,42%. O caminho polifônico com pesos, thresholds e resampler iguais aos da aplicação foi avaliado em três misturas artificiais de duas vozes: precisão80,40%, recall58,74%, F167,89%. Fontes, scripts, hashes, denominadores e limites nos dois documentos de benchmark. Não foi validada recuperação de naipes em coro real nem uma margem universal de latência.
