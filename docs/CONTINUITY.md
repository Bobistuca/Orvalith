---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 8 concluído como *partial* (ver matriz) → próximo: Estágio 9 (ajustes e gerenciamento de cor).
- **Funciona:** fmt, clippy -D warnings, 253 testes (núcleo 175 + app 78) e build release OK; app abre/fecha (smoke). Texto/formas/caneta/nós só com eventos sintéticos e testes de núcleo. Pintura com mouse real NÃO foi exercitada (só testes de núcleo e de `ToolState::handle` com eventos sintéticos). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view,paint,fill,geom,blend,compose,layerops,selection,transform,docops,snap,maskops}.rs`, `crates/orvalith/src/{tools,canvas,layers_ui,select,geometry}.rs`, `crates/orvalith/src/app/{io,selops,geocmds}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 9 — ajustes (brilho/contraste, níveis, curvas, matiz/saturação, balanço de cores etc.), camadas de ajuste não destrutivas, histograma e gerenciamento de cor (ICC, 16 bits se viável). Pendências: testes manuais com mouse/diálogos (Estágios 2–8); alças de forma e edição de texto no canvas; texto em caminho/vertical; jobs em segundo plano; camadas com deslocamento próprio.
