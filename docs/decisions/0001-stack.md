---
type: decisao
status: aceita
tags: [orvalith, decisao]
---
# 0001 — Stack inicial

- **Linguagem:** Rust estável 1.99.1 (`rust-toolchain.toml`).
- **UI:** egui/eframe 0.37 (backend padrão do eframe; o caminho de renderização real da janela será testado e documentado no Estágio 1).
- **Compositor:** CPU como referência de comportamento ([compose.rs](../../crates/orvalith-core/src/compose.rs)); GPU (wgpu) depois, com comparação por tolerância.
- **Histórico:** pilha de comandos reversíveis por documento.
- **Formato nativo:** `.orva`, contêiner versionado (Estágio 2).
- **Workspace:** apenas `orvalith-core` e `orvalith`; novos crates quando houver fronteira concreta.
- **Rede:** nenhuma no app ([[docs/LOCAL_STORAGE]]).
