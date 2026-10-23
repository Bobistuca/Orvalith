---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 6 concluído como *partial* (ver matriz) → próximo: Estágio 7 (máscaras, recorte e canais).
- **Funciona:** fmt, clippy -D warnings, 192 testes (núcleo 135 + app 57) e build release OK; app abre/fecha (smoke). Pintura com mouse real NÃO foi exercitada (só testes de núcleo e de `ToolState::handle` com eventos sintéticos). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view,paint,fill,geom,blend,compose,layerops,selection,transform,docops,snap}.rs`, `crates/orvalith/src/{tools,canvas,layers_ui,select,geometry}.rs`, `crates/orvalith/src/app/{io,selops,geocmds}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 7 — máscaras de camada raster (de seleção/preenchimento, pintar, inverter, ativar/desativar, aplicar, excluir, vincular posição), densidade/pena, visão isolada da máscara, máscaras de recorte (clipping), painel de canais (cor + alfa auxiliares; os canais salvos do Estágio 5 já persistem), refinar borda por algoritmos clássicos. A composição precisa ganhar máscara por camada (`compose.rs`) e `.orva` v5. Pendências: testes manuais com mouse/diálogos (Estágios 2–6); jobs em segundo plano; camadas com deslocamento próprio.
