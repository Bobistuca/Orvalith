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

## Pendente
Tiles, compositor GPU (wgpu), jobs em segundo plano (salvar/abrir ainda bloqueiam a UI).
