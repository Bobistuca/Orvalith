# Changelog

## Unreleased
- Estágio 6: transformação livre/inclinar/distorcer/perspectiva/malha com prévia e cancelamento, espelhar/girar camadas, alinhar/distribuir, recorte, endireitar, tamanho da imagem e da tela, girar/espelhar tela, interpolação (vizinho/bilinear/bicúbica), réguas, guias, grade, encaixe e rotação da visão; `.orva` v4 (guias).
- Estágio 5: seleções (retângulo, elipse, laço, laço poligonal, varinha, faixa de cores), combinação de modos, inverter/expandir/contrair/suavizar/pena, contorno animado, cortar/copiar/apagar/preencher e ferramentas respeitando a seleção, transformar seleção, canais de seleção salvos no `.orva` (v3) e máscara rápida.
- Estágio 4: árvore de camadas com grupos, 25 modos de mesclagem, opacidade/preenchimento, bloqueios, seleção múltipla, mesclar/achatar, Mover, copiar/colar entre documentos, miniaturas, isolamento; pixels das camadas compartilhados (`Arc`) para histórico barato; `.orva` v2.
- Estágio 3: pincel, lápis, borracha, conta-gotas, balde de tinta e gradientes; cores de frente/fundo e amostras; presets de pincel; camada ativa e nova camada; atualização parcial da textura; orçamento de memória do histórico.
- Estágio 2: abrir PNG/JPEG (EXIF), projeto `.orva` v1 versionado e validado, salvar/salvar como atômico, exportar PNG/JPEG, painel de camadas (visibilidade/opacidade) com desfazer/refazer, confirmação ao fechar, autosave/recuperação opcionais, recentes.
- Estágio 1: canvas com zoom/pan/ajustar/100%, abas com fechar e confirmação, atalhos sensíveis a foco, barra de status, preferências locais (tema, escala, janela). Núcleo: `view.rs` com testes.
- Estágio 0: workspace Rust, núcleo (documento, camadas, compositor CPU, histórico), janela eframe com estado vazio e "Novo documento", CI e documentação inicial.
