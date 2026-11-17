# Pesquisa de ferramentas para prática vocal

Consulta documental: **16/11/2026**, no fuso America/Fortaleza.

Esta pesquisa compara 12 produtos relacionados ao fluxo de carregar material próprio, seguir uma referência de notas e visualizar a voz durante a prática. Foram usadas páginas dos fabricantes, seus manuais e descrições publicadas pelos próprios desenvolvedores nas lojas. **Nenhum desses aplicativos foi executado ou medido nesta pesquisa.** Vídeos foram identificados nas páginas oficiais, mas não reproduzidos integralmente. Recursos podem variar por versão, plataforma e plano.

“Não documentado nas fontes consultadas” significa que a documentação lida não confirmou o recurso; não prova que o produto inteiro o desconheça. A lista é uma amostra direcionada, não um levantamento exaustivo do mercado. Não foram comparados preços.

## O que torna um produto comparável

- **Treino com alvo:** há uma referência temporal e a pessoa compara sua voz com ela durante a execução.
- **Monitor de pitch:** mostra o que a pessoa canta, mas pode não fornecer melodia alvo, acompanhamento ou avaliação de uma música.
- **Transcrição/edição:** extrai ou edita notas de uma gravação; isso não implica feedback de uma pessoa cantando ao vivo.
- **Preparação de repertório:** separa stems e oferece mixer, redução de velocidade ou loops; isso não implica avaliação vocal.

Essas categorias podem se sobrepor. Importar MIDI, MusicXML, uma sessão proprietária e um arquivo de áudio são capacidades diferentes.

## Comparação documental

| Produto | Papel confirmado | Material próprio e referência | Voz ao vivo e feedback | Relevância e limite para este projeto |
| --- | --- | --- | --- | --- |
| **Yousician — Singing** | Treino com alvo e pontuação | A documentação descreve prática de músicas do aplicativo, controle de tempo, transposição, voz guia e loop de trechos. Importar dois áudios próprios não foi confirmado nas páginas consultadas. | Reconhecimento da execução, pontos por afinação e timing; gravação para revisão. | Boa referência de transporte e feedback durante uma música; não comprova o fluxo de repertório próprio solicitado. [Practice/Play](https://support.yousician.com/hc/en-us/articles/360000553289-Practice-and-Play-modes-in-singing). |
| **Sing & See** | Monitor vocal e treino com curva de referência | Abre WAV para reprodução. Nas versões Professional/Teacher, uma gravação de exercício pode ser fixada como template. | O novo canto se sobrepõe à curva fixada, permitindo comparar modelo e execução. | É uma referência direta para **target + trace**, além de espectrograma. Mixer de duas faixas e pontuação temporal não foram confirmados. [Template](https://www.singandsee.com/help/pitchtemplate.htm), [arquivos e transporte](https://www.singandsee.com/help/toolbar.htm). |
| **Singscope** | Monitor vocal com apoio de partitura | O pacote de partitura importa um subconjunto de MusicXML lead sheets; toca as notas da partitura enquanto grava o canto, com tempo e transposição. | Curva de pitch em tempo real e posições das notas da partitura destacadas no gráfico; reprodução da voz para revisão. | Mostra que referência importada e voz podem compartilhar um gráfico. O desenvolvedor explicita limitação a uma voz, sem polifonia. Dois stems de áudio com mix não foram confirmados. [Descrição do desenvolvedor](https://apps.apple.com/us/app/singscope/id944309175). |
| **Singing Carrots** | Treino com alvo, exercícios próprios e estatísticas | **Importa MIDI próprio** em Custom Melodies. Se houver múltiplas trilhas, permite ouvir e escolher uma. A prática permite definir início/fim do trecho com flags na timeline. | Notas alvo e pitch ao vivo, revisão da gravação, controles de microfone, tempo e repetição. | Um dos comparáveis mais próximos para repertório próprio, porém a entrada documentada do alvo é MIDI, não extração automática de uma track vocal de áudio. [Custom Melodies](https://singingcarrots.com/docs/singing-carrots-custom-singing-exercises/), [interface de prática](https://singingcarrots.com/docs/pitch-training-practice-virtual-piano-interface). |
| **Nail the Pitch** | Monitor de pitch e revisão | Pro grava, reproduz, salva sessões e **exporta** melodias como MIDI. Exportação não deve ser confundida com importar um alvo MIDI ou áudio. | Piano roll da voz, zoom para slides/vibrato; teclado pode tocar notas de referência. | Boa referência de inspeção do pitch e navegação da própria execução. Alvo temporal importado, mixer de acompanhamento e score não foram confirmados. [Descrição e histórico do desenvolvedor](https://apps.apple.com/us/app/nail-the-pitch-vocal-monitor/id1486640549). |
| **Vocal Pitch Monitor / Pro — TadaoYamaoka** | Monitor de pitch e revisão | Gravação, reprodução e save/load. O changelog Android Pro anuncia importação de MP3, Ogg e outros áudios. | Gráfico com tempo e notas, escala e cores; a documentação informa que múltiplos instrumentos/acordes prejudicam a análise. | Importar áudio para inspecionar pitch é relevante, mas sobreposição target/live sincronizada e pontuação não foram confirmadas. As capacidades não devem ser presumidas iguais entre Android e iOS. [Android Pro, descrição e changelog](https://play.google.com/store/apps/details?hl=en&id=com.tadaoyamaoka.vocalpitchmonitornoad), [iOS](https://apps.apple.com/us/app/vocal-pitch-monitor/id842218231). |
| **PitchPop: Sing in Tune** | Monitor, exercícios, importação e comparação | Importa gravações de **uma voz sem acompanhamento**; Pro importa MIDI. Compara duas takes salvas, alinhadas pela primeira nota detectada, e alterna reprodução A/B. | Pitch, frequência, cents, waveform, gravação e exercícios com piano/Auto Loop. | Comparável importante para revisão e alinhamento. O desenvolvedor **exclui expressamente backing tracks, vozes simultâneas, instrumentos misturados e separação vocal** nesta versão. Comparar takes salvas não comprova acompanhar um áudio alvo próprio enquanto se canta ao vivo. [Descrição do desenvolvedor](https://apps.apple.com/us/app/pitchpop-sing-in-tune/id6801956244). |
| **Vox Tools** | Rotinas de treino vocal | Exercícios guiados, treinos personalizados, piano e gravador. O histórico documenta importação/exportação de rotinas no formato Vox Tools. | A descrição principal consultada não confirma gráfico target/live ou avaliação automática da afinação. | Referência para configurar exercícios e rotinas, com menor correspondência ao estudo de dois stems. Rotina proprietária não equivale a importar música em áudio. [Descrição e histórico do desenvolvedor](https://apps.apple.com/us/app/vox-tools-learn-to-sing/id568721281). |
| **Melodyne** | Análise e edição de gravações | Detecta e edita notas, afinação, timing e outros aspectos do áudio. DNA permite acesso a notas em material polifônico adequado. | Feedback de canto ao vivo contra target com score não foi confirmado; o foco documentado é análise/edição. | Referência forte para representação das notas e revisão manual. **DNA separa por pitch, não por instrumento ou identidade do cantor**; duas fontes na mesma nota não viram duas notas independentes. [Visão do produto](https://services.celemony.com/en/melodyne/what-is-melodyne), [algoritmos e limites de DNA](https://helpcenter.celemony.com/M5/doc/melodyneStudio5/en/M5tour_AudioAlgorithms). |
| **RipX DAW** | Separação, extração de notas, edição e aprendizado de músicas | Importa áudio, separa camadas e apresenta notas para estudar partes isoladas. Documentação de versões diferencia gravação de notas individuais e harmonias. | Gravação de áudio é documentada; avaliação automática de canto contra alvo com score não foi confirmada. | Referência para ouvir uma parte dentro do contexto e examinar suas notas. Extração de notas não garante separação perfeita das vozes de um acorde vocal. [Uso para aprender partes](https://hitnmix.com/2024/01/06/ripx-deepremix-deepcreate-for-guitarists/), [histórico oficial](https://hitnmix.com/changes/). |
| **AnthemScore** | Transcrição e edição de partitura | Importa áudio e gera notas; exporta PDF, MIDI e MusicXML. Pode transcrever notas de melodia cantada, sem transcrever a letra. | Treino de canto ao vivo com alvo e score não foi confirmado. | Pode preparar uma referência simbólica para treino, mas é uma etapa anterior ao exercício. Detecção de acordes e instrumentos não equivale a identificar cada backing vocal. [FAQ](https://lunaverus.com/faq), [primeiros passos](https://lunaverus.com/gettingStarted), [versão web](https://lunaverus.com/transcribe). |
| **Moises** | Preparação de repertório e prática com stems | Upload de áudio, separação, mix com mute/solo e volume, tempo, tom e loop de seções. O Premium documenta Lead Vocals e Background Vocals. | Nas páginas consultadas, não foi confirmado gráfico de melodia alvo comparado ao pitch do microfone nem score vocal. | Complementa diretamente o fluxo: preparar backing e voz alvo para importar aqui. Um stem Background Vocals pode conter várias vozes simultâneas. [Stems disponíveis](https://help.moises.ai/hc/en-us/articles/29530520103324-Which-instruments-can-be-separated-on-the-Premium-plan), [fluxo web/desktop e mixer](https://help.moises.ai/hc/en-us/articles/8472791352860-Welcome-to-Moises-Here-are-some-pointers-to-start-Web-Desktop). |

## Referências de UX que merecem orientar o trabalho

### Referência e execução no mesmo espaço musical

Sing & See documenta fixar a curva do modelo e desenhar a execução sobre ela. Singscope combina posições das notas de uma partitura com a curva da voz. Singing Carrots diferencia visualmente alvo e execução durante a prática. A conclusão de projeto é usar **uma escala musical e uma linha de tempo comuns**, evitando que a pessoa precise comparar gráficos separados. É uma inferência de design baseada nessas interfaces documentadas, não uma medição de eficácia pedagógica. [Sing & See](https://www.singandsee.com/help/pitchtemplate.htm), [Singscope](https://apps.apple.com/us/app/singscope/id944309175), [Singing Carrots](https://singingcarrots.com/docs/pitch-training-practice-virtual-piano-interface).

### Repetir uma frase sem reconstruir a sessão

Yousician documenta repetir uma região por sliders na barra de progresso. Singing Carrots oferece flags para início e fim de uma melodia importada. Moises permite voltar a seções da música. Para esta aplicação, isso sustenta loop A/B visível e ajustável junto da timeline; escolher o trecho e iniciar a prática deve exigir poucas ações. [Yousician](https://support.yousician.com/hc/en-us/articles/360000553289-Practice-and-Play-modes-in-singing), [Custom Melodies](https://singingcarrots.com/docs/singing-carrots-custom-singing-exercises/), [Moises](https://help.moises.ai/hc/en-us/articles/8472791352860-Welcome-to-Moises-Here-are-some-pointers-to-start-Web-Desktop).

### Ouvir e revisar continua importante

Nail the Pitch oferece gravação e revisão; PitchPop documenta comparação de takes com reprodução A/B na mesma posição; Singing Carrots oferece replay após a tentativa. Isso sugere evoluir o projeto para salvar a execução e ouvi-la sincronizada ao gráfico. Essa capacidade é uma oportunidade identificada, não uma afirmação sobre o que a v0 já implementa. [Nail the Pitch](https://apps.apple.com/us/app/nail-the-pitch-vocal-monitor/id1486640549), [PitchPop](https://apps.apple.com/us/app/pitchpop-sing-in-tune/id6801956244), [Singing Carrots](https://singingcarrots.com/docs/pitch-training-practice-virtual-piano-interface).

### Manter visível a origem do alvo

MIDI e MusicXML preservam alturas e tempos explícitos; análise de áudio precisa estimá-los. Melodyne oferece correção das decisões de detecção, incluindo casos em que um harmônico foi tratado como nota. A interface deve diferenciar a nota estimada da gravação, o contorno contínuo da referência e um alvo que a pessoa eventualmente revise. Importar símbolos pode ser uma alternativa futura para uma linha vocal difícil de extrair. [Detecção e revisão no Melodyne](https://helpcenter.celemony.com/M5/doc/melodyneStudio5/en/M5tour_NA_Mode_Tools), [MIDI próprio no Singing Carrots](https://singingcarrots.com/docs/singing-carrots-custom-singing-exercises/).

## Harmonias e backing vocals: o que a pesquisa sustenta

1. **Separar lead de backing vocals e separar cada voz de uma harmonia são tarefas diferentes.** Moises documenta stems Lead Vocals e Background Vocals; a página consultada não promete uma saída individual para cada cantor/altura dentro de um acorde vocal. [Fonte](https://help.moises.ai/hc/en-us/articles/29530520103324-Which-instruments-can-be-separated-on-the-Premium-plan).
2. **Reconhecer várias notas não identifica necessariamente quem canta cada uma.** A limitação documentada de DNA é justamente separar por altura, sem identidade da fonte. Não usar Melodyne como evidência de que um detector monofônico resolve automaticamente uma harmonia vocal. [Fonte](https://helpcenter.celemony.com/M5/doc/melodyneStudio5/en/M5tour_AudioAlgorithms).
3. **Uma linha por vez continua sendo um contrato útil.** Singscope explicita a restrição monofônica; Singing Carrots permite escolher uma trilha do MIDI. Para estudar backing vocals nesta v0, a entrada recomendada é a linha vocal isolada que a pessoa pretende cantar, com o restante no acompanhamento. É uma escolha de escopo do projeto, não uma limitação de todo software de áudio. [Singscope](https://apps.apple.com/us/app/singscope/id944309175), [seleção de trilha MIDI](https://singingcarrots.com/docs/singing-carrots-custom-singing-exercises/).
4. **Suporte polifônico futuro exige escolha do alvo.** Se uma referência contiver várias notas, o programa precisará representar múltiplas linhas e permitir selecionar qual deve ser avaliada. Escolher sempre a nota mais forte pode trocar entre as vozes; essa é uma consequência técnica a validar com exemplos reais, não um recurso confirmado nas ferramentas de treino pesquisadas.

## Demos, vídeos e materiais visuais identificados

Estes links permitem inspecionar o comportamento descrito. A pesquisa confirmou a presença dos materiais nas páginas dos fabricantes; não avaliou latência, precisão nem o fluxo completo em execução.

| Material | O que observar | Proveniência |
| --- | --- | --- |
| [Yousician: vídeo de loop](https://www.youtube-nocookie.com/embed/iIO9zXVR-nc) | Seleção e repetição de trecho | Vídeo incorporado no [artigo oficial Practice/Play](https://support.yousician.com/hc/en-us/articles/360000553289-Practice-and-Play-modes-in-singing). |
| [Sing & See: Pitch Target Template](https://www.singandsee.com/help/pitchtemplate.htm) | Capturas de modelo fixo e curva cantada por cima | Manual oficial com screenshots. |
| [Singing Carrots: Custom Melodies](https://singingcarrots.com/docs/singing-carrots-custom-singing-exercises/) | Demo Twinkle, Twinkle, Little Star, upload MIDI e flags do trecho | Documentação oficial; algumas funções exigem autenticação/plano. |
| [Singing Carrots: interface de prática](https://singingcarrots.com/docs/pitch-training-practice-virtual-piano-interface) | Alvo e pitch ao vivo, modo partitura, replay, som e tempo | Documentação oficial detalha os controles; não substitui teste de uso. |
| [Singscope na App Store](https://apps.apple.com/us/app/singscope/id944309175) | Capturas do gráfico e suporte de partitura | Ficha publicada pelo desenvolvedor. O site singscope.com não respondeu à consulta automatizada. |
| [RipX: How do vocal harmonies work?](https://www.youtube-nocookie.com/embed/Zf7Oh99Qp4k) | Exploração de harmonias e notas no editor | Incorporado na [galeria oficial](https://hitnmix.com/ripx-videos/). A galeria também inclui vídeos de terceiros; publicação nela não transforma opiniões de terceiros em claims do fabricante. |
| [RipX: For Learning Songs](https://www.youtube-nocookie.com/embed/nPnU2NTefA0) | Isolar partes e estudar notas durante reprodução | Incorporado na mesma galeria oficial; atenção a nomes legados de versões. |
| [Moises: introdução web/desktop](https://help.moises.ai/hc/en-us/articles/8472791352860-Welcome-to-Moises-Here-are-some-pointers-to-start-Web-Desktop) | Screenshots do mixer, mute/solo, tom, tempo e seções | Guia oficial. |

## Consequências para o projeto

A pesquisa confirma que o espaço já inclui treino com melodias próprias em formato simbólico, curvas de referência e comparação de takes. Portanto, não seria correto descrever a aplicação como uma ideia sem equivalentes.

O recorte específico deste projeto passa a ser **um áudio alvo próprio, backing opcional, mix sincronizado, escolha de trecho e visualização da voz durante a prática**. A documentação examinada não confirmou essa combinação completa em uma única ferramenta de treino da amostra. Isso é uma lacuna documental nesta pesquisa, não prova de exclusividade comercial.

A pesquisa foi usada na v0.2 para tornar a base opcional e introduzir candidatos polifônicos com escolha/revisão. Para os próximos passos, a ordem sugerida é:

1. Validar o fluxo atual com stems reais, incluindo uma linha de backing vocal isolada, e observar onde a detecção diverge do que a pessoa ouve.
2. Melhorar a revisão de trechos e da execução gravada antes de ampliar gamificação.
3. Acrescentar importação MIDI/MusicXML ou revisão manual do alvo quando a análise de áudio não for confiável.
4. Tratar múltiplas vozes como um modo separado, com seleção explícita da linha, em vez de prometer que um target monofônico representa qualquer harmonia.

Essas prioridades são inferências para este projeto a partir da pesquisa e do pedido do usuário; não são um ranking comprovado dos produtos.
