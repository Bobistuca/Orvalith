---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 5 concluído como *partial* (ver matriz) → próximo: Estágio 6 (transformações e geometria).
- **Funciona:** fmt, clippy -D warnings, 149 testes (núcleo 105 + app 44) e build release OK; app abre/fecha (smoke). Pintura com mouse real NÃO foi exercitada (só testes de núcleo e de `ToolState::handle` com eventos sintéticos). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view,paint,fill,geom,blend,compose,layerops,selection}.rs`, `crates/orvalith/src/{tools,canvas,layers_ui,select}.rs`, `crates/orvalith/src/app/{io,selops}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 6 — mover/escalar/girar/espelhar/transformação livre com pivô e valores numéricos, inclinar/distorcer/perspectiva, malha (warp), alinhar/distribuir, recortar/endireitar, redimensionar imagem e tela com âncora, réguas/guias/grade/encaixe, rotação de visão, interpolação (pixel art × foto). Pré-requisito: deslocamento/tamanho por camada (hoje camadas têm o tamanho do documento e o Mover desloca pixels). Pendências: testes manuais com mouse/diálogos (Estágios 2–5); jobs em segundo plano.
