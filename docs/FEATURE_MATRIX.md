---
type: matriz
status: ativo
tags: [orvalith, matriz]
---
# Matriz de recursos

| Recurso | Estágio | Estado | Evidência | Limitações |
|---|---|---|---|---|
| Janela nativa com tema | 0/1 | partial | build + execução | tema claro básico; sem ícone próprio |
| Novo documento (LxA, fundo) | 0/2 | partial | UI | sem resolução de impressão na UI |
| Modelo de documento/camada raster | 0 | partial | testes em `document.rs`/`history.rs` | lista plana, só RGBA8 |
| Compositor CPU (Normal + opacidade) | 0/4 | partial | testes `compose.rs` | sem modos de mesclagem |
| Histórico desfazer/refazer (núcleo) | 0/2 | partial | testes `history.rs` | não ligado à UI |
| Canvas: zoom (roda/Ctrl+= /Ctrl+-), ajustar (Ctrl+0), 100% (Ctrl+1), pan (botão do meio / Espaço+arrastar) | 1 | partial | testes de `view.rs` (ida e volta de coordenadas, âncora fixa, DPI); abertura do app | interação manual de mouse não exercitada; sem régua/rotação |
| Abas de documentos com fechar (×, Ctrl+W, clique do meio), Ctrl+Tab | 1 | partial | código + smoke test | confirmação de fechamento só descarta (salvar vem no Estágio 2) |
| Preferências (tema, escala da UI, tamanho da janela) | 1 | partial | testes de `prefs.rs` (ida e volta, arquivo corrompido, faixa) | sem atalhos/memória/autosave configuráveis |
| Atalhos sensíveis a foco de texto | 1 | partial | usa `egui_wants_keyboard_input` | sem teste automatizado de UI |
| Barra de status (zoom %, tamanho, coordenadas do cursor) | 1 | partial | código | — |
| Abrir PNG/JPEG (EXIF, limites de memória, aviso de redução de bits) | 2 | partial | testes de `imageio.rs`; diálogo e arrastar-e-soltar não exercitados manualmente | sem ICC; só 8 bits |
| Salvar/Salvar como/reabrir `.orva` | 2 | partial | testes de `orva.rs` e `io.rs` (ida e volta, corrupção, versão, bomba) | diálogos nativos não exercitados; leitura/gravação na thread da UI |
| Exportar PNG (alfa) e JPEG (qualidade, fundo) | 2 | partial | testes de `imageio.rs` | sem prévia/tamanho estimado, sem ICC |
| Escrita atômica | 2 | validated | `fsio.rs`: falha simulada preserva o arquivo anterior | — |
| Histórico ligado à UI (Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y) | 2 | partial | `edit.rs` testes; um arrasto = uma ação | sem painel visual de histórico |
| Indicador de modificado, confirmação ao fechar aba/sair (Salvar/Descartar/Cancelar) | 2 | partial | código | sem teste automatizado de UI |
| Autosave/recuperação opcional (desligado por padrão), apagar cópias | 2 | partial | `scan_recovery` testado; cópia em thread | autosave e recuperação ao iniciar não exercitados manualmente |
| Recentes (opcional, apagável) | 2 | partial | testes de `prefs.rs` | — |
| Pincel e lápis (tamanho, dureza, opacidade, fluxo, espaçamento), traço = 1 ação | 3 | partial | `paint.rs`: sem lacunas em traço rápido, opacidade por traço, fluxo, dureza, lápis sem antisserrilhado, recorte nas bordas, desfazer exato; `tools.rs`: arrasto via ferramenta | uso real com mouse não exercitado; CPU puro (sem medição de desempenho) |
| Borracha (em camada com transparência) | 3 | partial | `paint.rs` e `tools.rs` testes | — |
| Suavização do traço configurável | 3 | partial | teste `smoothing_still_reaches_final_point_on_finish` | média exponencial simples; sem estabilizador por corda |
| Pressão da caneta | 3 | partial | leitura de `Event::Touch.force` e escala de tamanho no núcleo (clamp) | **não verificada com tablet**; mouse sempre 100% |
| Conta-gotas (composto/camada ativa, Alt = fundo) | 3 | partial | `tools.rs` testes | — |
| Cores frente/fundo, trocar (X), padrão (D), seletor, amostras (padrão + do usuário) | 3 | partial | `tools.rs`, `prefs.rs` testes | seletor é o do egui (HSV/RGB, sem gerenciamento de cor) |
| Balde de tinta (tolerância, contíguo/global, opacidade) | 3 | partial | `fill.rs` + `tools.rs` testes | opera na camada ativa; sem seleção/antisserrilhado de borda |
| Gradientes linear/radial (frente→fundo, frente→transparente, inverter, opacidade) | 3 | partial | `fill.rs` testes (pontas, ponto médio, simetria, degenerado) + Esc cancela | 2 paradas (sem editor de paradas); sem dithering (faixas em 8 bits); histórico guarda a camada inteira |
| Presets de pincel (padrão + salvos pelo usuário) e amostras persistem | 3 | partial | testes de ida e volta de `prefs.rs` e `paint.rs` | — |
| Pintura persiste no projeto `.orva` | 3 | partial | teste `painted_pixels_survive_project_roundtrip` | — |
| Camada ativa + nova camada (desfazível) | 3/4 | validated | `layerops.rs`/`layers_ui.rs` testes | — |
| Atualização parcial da textura (só a região pintada) | 3 | partial | `compose::flatten_region` testado | custo não medido |
| Orçamento de memória do histórico (256 MiB) | 3 | partial | teste `budget_drops_oldest_actions` | descarta as ações mais antigas sem aviso |
| Criar, renomear (duplo clique), duplicar, excluir, reordenar (Subir/Descer) camadas | 4 | partial | `layerops.rs` + `layers_ui.rs` testes; cada operação = 1 ação de histórico | reordenar só por botões (sem arrastar); painel não exercitado com mouse |
| Grupos aninhados (agrupar, desagrupar, expandir/recolher, oculto/opacidade/modo) | 4 | partial | testes de `layerops.rs` e `compose.rs` | sem arrastar camadas para dentro/fora de grupos |
| Seleção múltipla (Ctrl/Shift+clique) | 4 | partial | `layers_ui.rs` | só uso por mesclar/agrupar/duplicar/excluir/mover/copiar |
| Bloqueios de posição, pixels e transparência | 4 | partial | testes de `paint.rs`, `fill.rs` e `tools.rs` | bloqueio de pixels vale para pincel/borracha/balde/gradiente (não para o Mover) |
| Opacidade e preenchimento separados | 4 | partial | `compose.rs` teste | idênticos até haver estilos de camada (Estágio 11) |
| 25 modos de mesclagem + passar através (grupos) | 4 | partial | `blend.rs` (fórmulas, identidades, faixa, não separáveis) e `compose.rs` | espaço sRGB codificado; sem comparação com outro programa; ver [[docs/COMPOSITING]] |
| Mesclar selecionadas, mesclar visíveis, achatar (com confirmação) | 4 | partial | testes de `layerops.rs` | difere do "mesclar para baixo" de referência com modos ≠ Normal |
| Ferramenta Mover (V) com preview e 1 ação; Esc cancela | 4 | partial | `tools.rs` testes | desloca pixels no buffer: o que sai da imagem é perdido (sem deslocamento por camada) |
| Copiar/colar camadas entre documentos (Ctrl+C/V, área interna) | 4 | partial | testes de `layerops.rs` e `layers_ui.rs` | não usa a área de transferência do sistema; colar centraliza se os tamanhos diferem |
| Miniaturas atualizadas | 4 | partial | teste de amostragem | adiadas durante traço; amostragem por vizinho mais próximo |
| Isolamento temporário de visibilidade (Alt+clique no olho) | 4 | partial | `compose.rs` teste | fora do histórico e do arquivo |
| Projeto `.orva` v2 (grupos, modos, bloqueios, preenchimento) lê também v1 | 4 | partial | testes de `orva.rs` | ver [[docs/ORVA_FORMAT]] |
| Comparação CPU×GPU de composição | 4 | blocked | — | não existe compositor GPU ainda |
| Seleção retangular e elíptica (M / Shift+M), proporção 1:1, bordas suaves ou duras | 5 | partial | `selection.rs` (área, cobertura fracionária, recorte nas bordas) + `select.rs` | uso com mouse não exercitado |
| Laço livre (L) e laço poligonal (Shift+L; duplo clique/Enter fecha, Esc cancela) | 5 | partial | `selection.rs` (polígono par-ímpar) + `select.rs` | regra par-ímpar para autointerseções |
| Varinha mágica (tolerância, contíguo, camada ativa ou todas) | 5 | partial | `select.rs` + `fill.rs` | bordas suavizadas por raio 1 se "Suavizar bordas" |
| Faixa de cores com prévia ao vivo e amostra do canvas | 5 | partial | `selection.rs`, `select.rs` | distância máx. por canal em RGB (sem Lab) |
| Modos nova/adicionar/subtrair/interseção (+ Shift, Alt, Shift+Alt) | 5 | partial | `selection.rs`, `select.rs` testes | união = máximo, interseção = mínimo, subtração proporcional |
| Selecionar tudo, desmarcar, inverter | 5 | partial | testes | — |
| Expandir, contrair, suavizar, pena (diálogo; pena também nas ferramentas) | 5 | partial | `selection.rs` testes (disco, borda do canvas, núcleo preservado) | síncrono; raio máx. 100 px |
| Antisserrilhado e contorno ("formigas") da seleção | 5 | partial | `selection_edges` | contorno desenhado só nas partes visíveis (máx. 20 000 trechos) |
| Cortar/copiar/apagar/preencher respeitando a seleção (inclusive suave) | 5 | partial | `selops.rs`, `selection.rs` testes | cortar não preenche com a cor de fundo; copiar usa área interna |
| Pincel, lápis, borracha, balde e gradiente limitados à seleção | 5 | partial | `tools.rs`: nada vaza para fora, borda suave dá efeito parcial, desfazer restaura | — |
| Transformar seleção (mover, escalar, girar a máscara) e mover seleção arrastando | 5 | partial | `selection.rs` (`transform`, `translate`) + `select.rs` | reamostragem bilinear; sem alças interativas |
| Salvar/carregar seleções como canais (persistem no `.orva` v3) | 5 | partial | `orva.rs` teste; `select.rs` | sem painel de canais completo (Estágio 7) |
| Desfazer/refazer de seleções e das edições com seleção | 5 | partial | `select.rs`, `selops.rs` testes | seleção não marca o documento como modificado |
| Máscara rápida (Q): pinte de preto/branco, sobreposição vermelha | 5 | partial | `selops.rs` teste de ida e volta | só pincel/lápis/balde/gradiente; sem opções de cor/opacidade da sobreposição |
| Transformação livre (Ctrl+T): mover, escalar, girar com valores numéricos, proporção travada, pivô (9 pontos), confirmar/cancelar | 6 | partial | `transform.rs` (identidade exata, deslocamento inteiro, espelho/90° exatos, sem halos de cor, área na rotação) + `geometry.rs` (1 ação, Esc restaura, seleção transformada junto) | alças com mouse não exercitadas; camadas ficam recortadas à tela (o que sai é perdido) |
| Inclinar, distorcer, perspectiva (homografia) | 6 | partial | `transform.rs` e `geometry.rs` testes | inclinar só numérico; sem alças de borda |
| Deformação por malha 4×4 (escopo documentado) | 6 | partial | `warp_mesh` identidade/deslocamento | triângulos com mapa afim por célula; borda externa sem suavização; sem malha configurável |
| Espelhar e girar 90°/180° camada(s) (exato, 1 ação) | 6 | partial | `geometry.rs` testes | — |
| Alinhar (6 modos) e distribuir camadas | 6 | partial | `docops.rs`, `geometry.rs` testes | usa limites do conteúdo opaco; meio pixel em larguras ímpares; ver Camada/Imagem |
| Recortar (ferramenta C, alças, além das bordas) e recortar para a seleção | 6 | partial | `docops.rs`, `geometry.rs` testes (inclui salvar/reabrir) | sem "ocultar pixels recortados" nem proporção fixa |
| Endireitar (gira e recorta ao maior retângulo inscrito) | 6 | partial | `docops.rs` testes (retângulo inscrito, sem cantos vazios) | por ângulo numérico; sem linha de horizonte no canvas |
| Tamanho da imagem (reamostragem, ppi) e tamanho da tela (âncora) | 6 | partial | `transform.rs`/`docops.rs` testes | filtro alargado ao reduzir; sem "reamostrar sem ppi" por unidade física na UI |
| Girar/espelhar a tela inteira (exato) | 6 | partial | `docops.rs` testes (invertível, guias acompanham) | — |
| Interpolação: vizinho mais próximo, bilinear, bicúbica (Catmull-Rom) | 6 | partial | `transform.rs` testes | sem Lanczos |
| Réguas (px/pol/cm), guias (criar arrastando, mover, remover), grade, encaixe | 6 | partial | `snap.rs`, `docops.rs` testes; guias persistem no `.orva` v4 | uso com mouse não exercitado; régua some com a visão girada; Mover não encaixa |
| Rotação da visão (±15°, redefinir) sem alterar a imagem | 6 | partial | `view.rs` testes (ida e volta, âncora, pan) | desenho girado não exercitado visualmente |
| Estágio 6 depende de camadas do tamanho da tela | 6 | partial | — | transformações são destrutivas e recortadas à tela; deslocamento por camada e objetos inteligentes ficam para o Estágio 11 |
| Máscaras de camada raster (e de grupo): criar (revelar/ocultar/da seleção/seleção invertida), pintar, inverter, ativar, aplicar, excluir | 7 | partial | `maskops.rs` (composição, pixels preservados, aplicar = mesmo resultado), `tools.rs` (pincel, lápis, borracha, balde, gradiente na máscara; 1 ação), `layers_ui.rs` | uso com mouse/painel não exercitado; máscara é do tamanho da tela |
| Vincular/desvincular máscara e camada ao mover/transformar/alinhar | 7 | partial | testes em `docops.rs`, `tools.rs`, `geometry.rs` | grupos: a máscara do grupo não acompanha o conteúdo ao mover |
| Densidade e pena da máscara (efetivas, em cache) e visão isolada da máscara | 7 | partial | `maskops.rs`, `compose.rs` testes; arrasto = 1 ação | pena por desfoque gaussiano aproximado em toda a tela |
| Máscara → seleção; seleção → máscara | 7 | partial | `maskops.rs`, `layers_ui.rs` testes | — |
| Máscaras de recorte (clipping) em cadeia, com modo/opacidade/máscara próprios e base com máscara ou grupo | 7 | partial | `compose.rs`: alfa da base manda, opacidade da base vale para a cadeia, ocultar base oculta a cadeia, grupos | diferenças de referência no alfa parcial (aproximação); ver [[docs/COMPOSITING]] |
| Painel de canais: visualizar R/G/B (cinza ou combinado), canais alfa salvos (ver, selecionar, renomear, excluir, editar na máscara rápida) | 7 | partial | `canvas.rs`, `select.rs`, `selops.rs` testes | só RGB 8 bits; sem CMYK/Lab/canais de cor por camada |
| Refinar borda (suavizar, deslocar, pena, contraste) para máscara e seleção | 7 | partial | `maskops.rs` testes | algoritmos clássicos simples; sem detecção de cabelo/raio inteligente (fora do escopo, sem IA) |
| Máscaras vetoriais | 8 | partial | `vecops.rs` teste (forma → máscara, persiste no `.orva` v6, vira raster ao pintar) | só a partir de uma forma/caminho; edição dos nós da máscara depois de criada não existe |
| Texto de ponto e de parágrafo editáveis (família, negrito/itálico, tamanho, cor, alinhamento, entrelinha, espaçamento) | 8 | partial | `text.rs` (cosmic-text: modelagem Unicode, fallback, acentos; testes pulam sem fontes do sistema), `vectool.rs` | editor é uma caixa flutuante, não edição no próprio canvas; sem rich text por trecho; uma fonte por camada |
| Fontes do sistema, busca e fonte ausente (aviso ao reabrir, aparência gravada mantida, substituta ao editar) | 8 | partial | `text.rs`, `vecops::missing_fonts` testes | substituta é a padrão do sistema; sem pré-visualização de fontes |
| Formas (retângulo, arredondado, elipse, polígono, estrela, linha) com preenchimento sólido/gradiente e traço editáveis | 8 | partial | `vector.rs` (áreas, AA, JSON), `vecops.rs`, `layers_ui.rs` testes | sem alças de redimensionar no canvas (valores no painel); sem traço tracejado |
| Caneta Bézier e seleção de nós (inserir, mover, alças, excluir, abrir/fechar) | 8 | partial | `vector.rs` testes de caminho, `vectool.rs` testes com eventos sintéticos | mouse real não exercitado |
| Operações booleanas (união, subtração, interseção, exclusão) entre formas | 8 | partial | `vector.rs` áreas, `layers_ui.rs` teste | resultado por cobertura (aritmética de alfa), não contorno vetorial exato; operandos guardados |
| Caminho → seleção; rasterização explícita (única forma de pintar/transformar camada de texto/forma) | 8 | partial | `vecops.rs`, `layers_ui.rs` testes | transformar/pintar exige rasterizar antes (aviso) |
| Texto em caminho; texto vertical | 8 | planned | — | não implementados |
| `.orva` v6: origem editável (texto/forma) e máscara vetorial persistem; origem corrompida vira raster | 8 | partial | `orva.rs` testes de ida e volta e origem hostil | ver [[docs/ORVA_FORMAT]] |
| `.orva` v5: máscaras e recorte persistem | 7 | partial | `orva.rs` teste de ida e volta + corrupção | ver [[docs/ORVA_FORMAT]] |
| Ajustes: brilho/contraste, exposição, níveis (por canal), curvas (spline monotônica por canal), matiz/saturação, vibração, equilíbrio de cores, preto e branco, filtro de foto, mixagem de canais, cor seletiva, mapa de gradiente, inverter, posterizar, limiar, tons de cinza | 9 | partial | `adjust.rs` testes (identidade exata em níveis/curvas, valores conhecidos, alfa preservado, parâmetros hostis saneados) | cálculo em `f32` sobre RGB 8 bits sRGB codificado; luminância Rec.601; cor seletiva e equilíbrio são modelos clássicos aproximados (não idênticos a outros editores); sem "colorir" em matiz/saturação |
| Ajuste destrutivo com prévia ao vivo, respeitando a seleção (uma ação de desfazer; cancelar restaura) | 9 | partial | `adjustui.rs` testes (prévia, aplicar, cancelar, desfazer), `adjops.rs` (seleção parcial) | só camadas raster comuns (texto/forma exigem rasterizar); janela não exercitada com mouse |
| Camadas de ajuste não destrutivas (opacidade, modo, máscara, recorte, grupos) | 9 | partial | `adjops.rs`/`compose.rs` testes (opacidade, máscara, recorte, modo, mesclar embute, alfa intacto) | agem sobre tudo abaixo no mesmo grupo; camada guarda pixels transparentes do tamanho da tela; editor no painel da camada não exercitado com mouse |
| Histogramas de luminosidade e de canais R/G/B/RGB (imagem ou camada, com seleção) | 9 | partial | `histogram.rs` testes, `adjustui.rs` | 8 bits; recalculado a cada revisão com a janela aberta (sem cache incremental) |
| Tons de cinza (ajuste) | 9 | partial | `adjust.rs` valor conhecido | não há modo de documento em cinza; o documento segue RGB |
| Profundidade de bits: 8 bits por canal | 9 | partial | `imageio.rs` aviso ao reduzir 16 bits | 16 bits e ponto flutuante NÃO suportados (reduzidos a 8 bits com aviso) |
| Perfis ICC: preservar na abertura (PNG/JPEG) e no `.orva` v7, embutir na exportação, atribuir x converter, converter para sRGB ao exportar | 9 | partial | `color.rs` (sRGB→P3 conhecido, ida e volta Adobe RGB, alfa), `imageio.rs`, `orva.rs`, `adjustui.rs` testes | só perfis RGB; perfis de cinza/CMYK são descartados com aviso; camadas de ajuste e máscaras não são convertidas; conversão rasteriza texto/forma |
| Prévia na tela convertida do perfil do documento para sRGB | 9 | partial | `color.rs` `Preview` teste | presume tela sRGB (sem perfil do monitor); desligável no diálogo Perfil de cor |
| 16 bits, ponto flutuante, Lab, CMYK, prova de cores (soft proof) | 9 | planned | — | não implementados; declarado |
| `.orva` v7: ajustes e perfil persistem; ajuste inválido vira camada transparente; bytes do perfil preservados | 9 | partial | `orva.rs` testes de ida e volta, ajuste hostil, CRC do perfil, leitura de v6 | ver [[docs/ORVA_FORMAT]] |
| Filtros: gaussiano, caixa, movimento, máscara de nitidez, nitidez, reduzir ruído (bilateral), mediana, ruído com semente, pixelar, relevo, bordas, redemoinho, ondulação, esferizar | 10 | partial | `filter.rs` testes (imagem plana estável, sem halos em transparência, energia do impulso, bordas, semente reprodutível, cancelamento, seleção, parâmetros hostis) | `f32` pré-multiplicado (≈16 B/px de trabalho); sem cache incremental; relevo/bordas devolvem cinza; esferizar/redemoinho/ondulação são distorções básicas centradas na camada |
| Prévia de filtro em segundo plano, cancelável, respeitando a seleção, 1 ação de desfazer | 10 | partial | `filterui.rs` testes (thread, reiniciar ao mudar parâmetros, cancelar restaura, seleção, bloqueios) | a janela não foi exercitada com mouse; só há um filtro por vez |
| Carimbo de clone (origem por Alt+clique, alinhado, camada ativa ou composta) e pincel de correção | 10 | partial | `retouch.rs` (cópia por deslocamento, sem halos, seleção, correção mantém o tom do local), `retouchui.rs` (traço = 1 ação, Esc cancela) | correção usa a média do anel ao redor (corrige tom, não gradientes fortes); sem amostragem de várias origens |
| Correção pontual (origem automática pelo anel mais parecido) e remendo (seleção + arrasto) | 10 | partial | `retouch.rs` testes (mancha em gradiente, remendo) | busca em 64 candidatos, sem sintetizar textura; sem prévia do remendo ao arrastar |
| Olhos vermelhos | 10 | partial | `retouch.rs` teste (pupila vermelha escurece, íris marrom não) | heurística de cor; sem detecção de olhos |
| Pincéis de desfocar, nitidez, borrar, subexpor, superexpor (sombras/meios-tons/realces) e esponja | 10 | partial | `retouch.rs` testes por pincel e borrar arrastando cor | sem pressão; alfa bloqueado restaurado só ao fim do traço |
| Preenchimento por textura / inpainting clássico | 10 | planned | — | não implementado (a correção pontual copia de uma origem, não é inpainting) |
| Liquify: deformar, suavizar e reconstruir (campo de deslocamento sobre o original; Enter aplica, Esc cancela; respeita a seleção) | 10 | partial | `liquify.rs` (identidade exata, empurrar, reconstruir volta ao original, suavizar, seleção, sem halos), `liquifyui.rs` (sessão, 1 ação, Esc, reconstruir, bloqueios) | só a camada ativa; sem inchar/contrair/congelar/máscara de congelamento; campo ocupa 8 B/px; renderização parcial não foi medida em imagens grandes; mouse real não exercitado |
| Estilos de camada: sombra projetada/interna, brilho externo/interno, traço (fora/centro/dentro), sobreposição de cor/gradiente/padrão, chanfro e relevo; modo e opacidade por efeito; ativar, reordenar, remover | 11 | partial | `style.rs` testes por efeito (posição, recorte ao alfa, sem halos em translucidez), `compose.rs` testes (ligar/desligar restaura, região parcial = render completo, opacidade e máscara) | só camadas raster/texto/forma (não grupos nem camadas recortadas); borda do traço por limiar 0,5 do alfa; chanfro por campo de altura suavizado (aproximação); desfoque por três caixas; sem contorno/curva de brilho, sem textura no chanfro; efeitos não foram exercitados no painel com mouse |
| Camadas de preenchimento (cor sólida, gradiente linear/radial, padrão procedural) editáveis | 11 | partial | `source.rs`/`orva.rs`/`layers_ui.rs` testes | são camadas com origem editável (rasterizar descarta); padrões procedurais (xadrez, listras, pontos), sem padrões de arquivo |
| Predefinições de estilo (salvar, aplicar, excluir) nas preferências locais | 11 | partial | `styleui.rs` teste do fluxo de gravação; preferências saneadas | só estilos; sem predefinições de outros recursos |
| `.orva` v8: estilos persistem; estilos fora dos limites são ignorados | 11 | partial | `orva.rs` testes de ida e volta e estilo hostil | ver [[docs/ORVA_FORMAT]] |
| Objetos inteligentes embutidos: original preservado, transformação reversível (posição, escala, giro) e conversão de camada raster | 11 | partial | `object.rs` testes (render idêntico ao converter, escala 0,25→1 reconstrói o original exato, translação sem reamostragem, recorte mantém e redimensionar rasteriza), `orva.rs` | transformação por valores numéricos no painel (sem alças nem Ctrl+T em objetos); o original é RGBA8 |
| Objetos vinculados: caminho, detecção de arquivo ausente, atualizar, reencontrar e embutir | 11 | partial | `object.rs` testes com arquivos temporários (atualizar, ausente → aviso e aparência mantida, embutir), vínculos de rede/URL recusados | caminho absoluto (sem caminho relativo ao projeto); só PNG/JPEG; sem monitorar mudanças do arquivo (atualização manual) |
| Filtros inteligentes: lista editável, ligar/desligar, parâmetros persistidos, máscara própria (da seleção) | 11 | partial | `object.rs` e `orva.rs` testes (editar parâmetros, desligar restaura o original, máscara limita, reabrir e editar) | rodam sincronamente no original a cada mudança (lento em imagens grandes); a máscara não é pintável (vem da seleção); sem reordenar |
| `.orva` v9: objetos embutidos gravam o original; vinculados só o caminho; entradas inválidas são ignoradas | 11 | partial | `orva.rs` testes (ida e volta, vinculado sem original, rede descartada, bomba de dimensões, CRC) | ver [[docs/ORVA_FORMAT]] |
| Composições de camadas (visibilidade, opacidade, preenchimento, modo e estilos ativos): criar, aplicar, atualizar, excluir | 11 | partial | `comps.rs` testes (capturar/aplicar, camadas removidas/novas, desfazer, valores hostis), `compsui.rs` (cada ação é desfazível), `orva.rs` | não guarda posição nem pixels (limitação documentada); sem comentário por composição |
| `.orva` v10: composições persistem; listas exageradas são descartadas | 11 | partial | `orva.rs` teste | ver [[docs/ORVA_FORMAT]] |
| Abrir/exportar WebP (sem perdas na gravação), GIF/BMP/TIFF/TGA/ICO/QOI/PNM estáticos com alfa onde o formato permite | 12 | partial | `formats.rs` testes (ida e volta exata de pixels/alfa, tolerâncias de formatos com perdas, detecção por conteúdo, animados → 1º quadro com aviso) | veja a tabela real em [[docs/FORMAT_SUPPORT]] |
| HDR/EXR (conversão para 8 bits com aviso), SVG (rasterizado com aviso), OpenRaster (camadas/grupos/opacidade/modos) | 12 | partial | `formats.rs` testes (SVG com alfa, ORA com contêiner válido, composição idêntica, arquivos hostis) | sem alto alcance dinâmico real; SVG não fica vetorial; ORA sem máscaras/estilos |
| Exportação: formato, qualidade, fundo, lado máximo, perfil, avisos, estimativa de tamanho, lote (documentos, camadas, folha de sprites) | 12 | partial | `formats.rs` testes (avisos, miniaturas, sprites); janela não exercitada com mouse | sem impressão com dimensões físicas |
| AVIF, HEIC/HEIF, JPEG XL, JPEG 2000, Camera RAW | 12 | planned | — | recusados com o motivo (dependências nativas/licenciamento); nenhum modelo de câmera testado |
| PDF: importar páginas (miniaturas, escolha, resolução, fundo, recorte, senha local) como imagens | 12 | partial | `pdf.rs` testes (renderização, recorte, fundo, limite, senha) | rasterizado: texto não editável; AES/RC4-40 sem arquivos de teste; renderizador não cobre todos os PDFs |
| PDF: exportar PDF raster multipágina (tamanho, orientação, margens, resolução efetiva, Flate com alfa ou JPEG) | 12 | partial | `pdf.rs` testes com leitores independentes (lopdf e hayro) | sem texto/vetores preservados; sem PDF/A |
| Photoshop PSD/PSB: camadas raster, grupos, máscaras, opacidade, modos, recorte, perfil, resolução; 8/16 bits na leitura | 12 | partial | `psd.rs` testes (ida e volta PSD/PSB, leitor independente `psd`, raw/RLE/ZIP/16 bits/cinza, arquivos hostis) | texto/forma/ajuste/estilo/objeto não são editáveis (pixels ou omitidos, com aviso); escrita 8 bits RGB; Photoshop real não testado; CMYK/Lab/Indexado recusados |
| PhotoCraft `.pcraft`: leitura do manifesto versão 1 (camadas raster, grupos, máscaras, modos, perfil) | 12 | partial | `pcraft.rs` testes com arquivos sintéticos (hash adulterado, versão, modo, aninhamento, truncamento) | somente leitura; escrita não implementada (esquema público incompleto); PhotoCraft real não testado |
| Interoperabilidade documentada (leitores independentes, pendências explícitas) | 12 | partial | [[docs/FORMAT_SUPPORT]] | validação em aplicativos reais pendente; sem impressão |
| Demais estágios (13–16) | 13–16 | planned | — | ver [[ROADMAP]] |
