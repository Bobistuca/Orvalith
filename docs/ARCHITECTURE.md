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

## Pendente
Tiles, compositor GPU (wgpu), jobs em segundo plano, formato `.orva` (Estágio 2).
