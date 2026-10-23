---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 4 concluído como *partial* (ver matriz) → próximo: Estágio 5 (seleções).
- **Funciona:** fmt, clippy -D warnings, 112 testes (núcleo 86 + app 26) e build release OK; app abre/fecha (smoke). Pintura com mouse real NÃO foi exercitada (só testes de núcleo e de `ToolState::handle` com eventos sintéticos). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view,paint,fill,geom,blend,compose,layerops}.rs`, `crates/orvalith/src/{tools,canvas,layers_ui}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 5 — seleções (retângulo, elipse, laço, varinha, faixa de cores, modos de combinação, inverter/expandir/contrair/suavizar, pena, máscara rápida), com cortar/copiar/preencher respeitando a seleção; um modelo de máscara de seleção em `orvalith-core` que as ferramentas de pintura/balde/gradiente passem a respeitar. Pendências: testes manuais com mouse/diálogos (Estágios 2–4); jobs em segundo plano; deslocamento por camada.
