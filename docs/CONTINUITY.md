---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 3 concluído como *partial* (ver matriz) → próximo: Estágio 4 (camadas e composição).
- **Funciona:** fmt, clippy -D warnings, 70 testes (núcleo 54 + app 16) e build release OK; app abre/fecha (smoke). Pintura com mouse real NÃO foi exercitada (só testes de núcleo e de `ToolState::handle` com eventos sintéticos). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view,paint,fill,geom}.rs`, `crates/orvalith/src/{tools,canvas}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 4 — grupos, renomear/duplicar/excluir/reordenar camadas, bloqueios, modos de mesclagem (lista completa com testes de fórmula), mesclar/achatar, miniaturas. Pendências: teste manual de diálogos/autosave/recuperação (Estágio 2) e de pintura com mouse (3); jobs em segundo plano para salvar/abrir.
