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
| Histórico ligado à UI (Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y) com visibilidade e opacidade de camada | 2 | partial | testes de `edit.rs`; um arrasto = uma ação | só propriedades de camada existem como ações |
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
| Camada ativa + nova camada (desfazível) | 3/4 | partial | `edit.rs` testes | sem renomear/excluir/reordenar (Estágio 4) |
| Atualização parcial da textura (só a região pintada) | 3 | partial | `compose::flatten_region` testado | custo não medido |
| Orçamento de memória do histórico (256 MiB) | 3 | partial | teste `budget_drops_oldest_actions` | descarta as ações mais antigas sem aviso |
| Demais estágios (4–16) | 4–16 | planned | — | ver [[ROADMAP]] |
