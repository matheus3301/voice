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
