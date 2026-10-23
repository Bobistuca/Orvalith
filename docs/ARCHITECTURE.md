---
type: arquitetura
status: ativo
tags: [orvalith, arquitetura]
---
# Arquitetura

Decisão base: [[docs/decisions/0001-stack]].

## Crates
- `orvalith-core`: [document.rs](../crates/orvalith-core/src/document.rs) (Document, Layer, IDs, revisão), [compose.rs](../crates/orvalith-core/src/compose.rs) (compositor CPU de referência), [history.rs](../crates/orvalith-core/src/history.rs) (comandos reversíveis).
- `orvalith` (app): [app.rs](../crates/orvalith/src/app.rs) UI, [theme.rs](../crates/orvalith/src/theme.rs) identidade visual.

## Convenções
- Coordenadas em pixels do documento, independentes de zoom/DPI.
- Pixels RGBA8, sRGB, alfa **não pré-multiplicado**; conversões nas bordas de import/export.
- `Document::revision` avança a cada edição para descartar resultados assíncronos obsoletos.
- Histórico por documento; cada `Command` é uma ação atômica (um traço = um comando).
- Precisão interna futura: evoluir para 16 bits/float sem converter silenciosamente para 8 bits (pendente).

## Arquivos (Estágio 2)
[orva.rs](../crates/orvalith-core/src/orva.rs) (formato, [[docs/ORVA_FORMAT]]), [imageio.rs](../crates/orvalith-core/src/imageio.rs) (PNG/JPEG), [fsio.rs](../crates/orvalith-core/src/fsio.rs) (escrita atômica), [edit.rs](../crates/orvalith-core/src/edit.rs) (comandos de camada). `History::record` registra uma ação já aplicada (arrasto de controle = uma ação). UI de arquivos em [io.rs](../crates/orvalith/src/app/io.rs).

## Pintura (Estágio 3)
[paint.rs](../crates/orvalith-core/src/paint.rs): `StrokeSession` carimba discos espaçados ao longo do caminho; cobertura do traço em buffer próprio (fluxo por carimbo) e opacidade aplicada uma vez sobre o original da camada; no fim vira um `PixelPatch` (uma ação). [fill.rs](../crates/orvalith-core/src/fill.rs): balde (varredura por linhas) e gradientes. [tools.rs](../crates/orvalith/src/tools.rs) interpreta a entrada; [canvas.rs](../crates/orvalith/src/canvas.rs) devolve eventos de ponteiro (todas as amostras do quadro, em coordenadas do documento) e atualiza a textura só na região alterada (`mark_dirty`/`record_patch`).

## Camadas (Estágio 4)
`Layer.pixels` é `Arc<Vec<u8>>` (copy-on-write via `pixels_mut`): clonar a lista de camadas é barato, e `edit::LayersEdit` guarda antes/depois da lista inteira — operações estruturais (agrupar, mesclar, mover, renomear, bloqueios…) são uma ação cada, e só buffers novos contam para o orçamento do histórico. [layerops.rs](../crates/orvalith-core/src/layerops.rs) são funções puras que devolvem a nova lista (invariante: pré-ordem, `normalize`). Detalhes de composição em [[docs/COMPOSITING]]. UI: [layers_ui.rs](../crates/orvalith/src/layers_ui.rs).

## Seleções (Estágio 5)
[selection.rs](../crates/orvalith-core/src/selection.rs): máscaras de 8 bits (255 = selecionado) com formas (retângulo, elipse, polígono), varinha, faixa de cores, morfologia (disco), pena, suavização, transformação e as operações `clear/fill/extract_selected`. O documento guarda `selection` e `channels` (com `Arc`); alterações passam por `edit::SelectionEdit` (histórico, sem sujar o documento nem reenviar a textura). Pincel, balde e gradiente recebem a máscara e misturam o resultado pixel a pixel. UI: [select.rs](../crates/orvalith/src/select.rs) (ferramentas e diálogos) e [selops.rs](../crates/orvalith/src/app/selops.rs) (cortar/copiar/preencher/máscara rápida). A máscara rápida é um documento cinza próprio, editado pelas mesmas ferramentas, com histórico separado até sair.

## Geometria (Estágio 6)
[transform.rs](../crates/orvalith-core/src/transform.rs): `Affine`, `Homography`, amostragem (vizinho/bilinear/bicúbica) em alfa pré-multiplicado, `warp*` (paralelo por linhas) e `resample` separável com filtro alargado ao reduzir. [docops.rs](../crates/orvalith-core/src/docops.rs): `DocState`/`DocEdit` (estado geométrico inteiro, pixels por `Arc`), recorte, tela, imagem, orientação, endireitar, alinhar/distribuir. [snap.rs](../crates/orvalith-core/src/snap.rs) e rotação em [view.rs](../crates/orvalith-core/src/view.rs). UI: [geometry.rs](../crates/orvalith/src/geometry.rs) (sessão de transformação com prévia viva, ferramenta de recorte, diálogos) e [canvas.rs](../crates/orvalith/src/canvas.rs) (réguas, guias, grade, malha de textura girável).

## Pendente
Tiles, compositor GPU (wgpu), jobs em segundo plano (salvar/abrir ainda bloqueiam a UI).
