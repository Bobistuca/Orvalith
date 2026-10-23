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
| Demais estágios (3–16) | 3–16 | planned | — | ver [[ROADMAP]] |
