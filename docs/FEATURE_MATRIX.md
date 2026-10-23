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
| Demais estágios (5–16) | 5–16 | planned | — | ver [[ROADMAP]] |
