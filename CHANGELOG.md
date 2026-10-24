# Changelog

## Unreleased
- Estágio 14: vídeo e áudio básicos com FFmpeg externo isolado (metadados reais, prévia por quadro, trilhas, cortes e aparo arrastando, títulos/imagens, fade/dissolver, mixagem de áudio, exportação MP4/WebM com progresso e cancelamento, mídia relativa/relink); `.orva` v12; docs/MEDIA.md.
- Estágio 13: linha do tempo (quadro a quadro, quadros-chave com interpolação, pele de cebola, prévia com indicador de taxa), exportação em GIF animado e sequência PNG; `.orva` v11.
- Estágio 12.3–12.5: PSD/PSB (leitura e escrita próprias, camadas/grupos/máscaras), leitura de `.pcraft` (manifesto v1, só leitura) e matriz de interoperabilidade com evidências e pendências.
- Estágio 12.2: PDF local — importar páginas rasterizadas (senha, resolução, fundo, recorte) e exportar PDF raster multipágina.
- Estágio 12.1: WebP, GIF, BMP, TIFF, TGA, ICO, QOI, PNM, HDR, EXR, SVG (rasterizado) e OpenRaster; janela de exportação com formato, avisos, estimativa de tamanho, miniaturas e lote (documentos, camadas, sprites); tabela real em docs/FORMAT_SUPPORT.md.
- Estágio 11.3: composições de camadas; `.orva` v10.
- Estágio 11.2: objetos inteligentes (embutidos e vinculados) com transformação reversível, filtros inteligentes com máscara própria; `.orva` v9.
- Estágio 11.1: estilos de camada (9 efeitos), camadas de preenchimento (cor, gradiente, padrão), predefinições de estilo; `.orva` v8.
- Estágio 10.2: Liquify (deformar, suavizar, reconstruir) com campo de deslocamento, prévia ao vivo, Enter/Esc e uma ação de desfazer.
- Estágio 10.1: menu Filtro (14 filtros clássicos com prévia em segundo plano, cancelável e respeitando a seleção) e ferramentas de retoque (carimbo de clone/correção, correção pontual, remendo, olhos vermelhos, desfocar/nitidez/borrar/subexpor/superexpor/esponja). Liquify fica para a subetapa 10.2.
- Estágio 9: 16 ajustes de cor (destrutivos com prévia e como camadas de ajuste editáveis), histogramas, perfis ICC (preservar, atribuir, converter, prévia sRGB, embutir/converter na exportação) com `moxcms`; `.orva` v7. Sem 16 bits/Lab/CMYK/prova de cores.
- Estágio 8: texto de ponto/parágrafo editável (fontes do sistema, Unicode, fallback, fonte ausente), formas com preenchimento/gradiente/traço, caneta Bézier e seleção de nós, booleanas, caminho → seleção, máscara vetorial e rasterização explícita; `.orva` v6 (`source`, máscara vetorial). Texto em caminho e vertical ficam planejados.
- Estágio 7: máscaras de camada (criar, pintar, inverter, ativar, aplicar, excluir, vincular, densidade, pena, visão isolada), máscaras de recorte em cadeia, painel de canais (R/G/B e canais alfa salvos, editar pela máscara rápida), refinar borda; `.orva` v5.
- Estágio 6: transformação livre/inclinar/distorcer/perspectiva/malha com prévia e cancelamento, espelhar/girar camadas, alinhar/distribuir, recorte, endireitar, tamanho da imagem e da tela, girar/espelhar tela, interpolação (vizinho/bilinear/bicúbica), réguas, guias, grade, encaixe e rotação da visão; `.orva` v4 (guias).
- Estágio 5: seleções (retângulo, elipse, laço, laço poligonal, varinha, faixa de cores), combinação de modos, inverter/expandir/contrair/suavizar/pena, contorno animado, cortar/copiar/apagar/preencher e ferramentas respeitando a seleção, transformar seleção, canais de seleção salvos no `.orva` (v3) e máscara rápida.
- Estágio 4: árvore de camadas com grupos, 25 modos de mesclagem, opacidade/preenchimento, bloqueios, seleção múltipla, mesclar/achatar, Mover, copiar/colar entre documentos, miniaturas, isolamento; pixels das camadas compartilhados (`Arc`) para histórico barato; `.orva` v2.
- Estágio 3: pincel, lápis, borracha, conta-gotas, balde de tinta e gradientes; cores de frente/fundo e amostras; presets de pincel; camada ativa e nova camada; atualização parcial da textura; orçamento de memória do histórico.
- Estágio 2: abrir PNG/JPEG (EXIF), projeto `.orva` v1 versionado e validado, salvar/salvar como atômico, exportar PNG/JPEG, painel de camadas (visibilidade/opacidade) com desfazer/refazer, confirmação ao fechar, autosave/recuperação opcionais, recentes.
- Estágio 1: canvas com zoom/pan/ajustar/100%, abas com fechar e confirmação, atalhos sensíveis a foco, barra de status, preferências locais (tema, escala, janela). Núcleo: `view.rs` com testes.
- Estágio 0: workspace Rust, núcleo (documento, camadas, compositor CPU, histórico), janela eframe com estado vazio e "Novo documento", CI e documentação inicial.
