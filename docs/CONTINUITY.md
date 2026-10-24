---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 13 (animação e linha do tempo) concluído como *partial* → próximo: Estágio 14 (vídeo e áudio básicos, só depois de estabilizar a base de imagem e animação).
- **Funciona:** fmt, clippy -D warnings, 386 testes (núcleo 287 + app 99) e build release OK; app abre/fecha (smoke). Texto/formas/caneta/nós só com eventos sintéticos e testes de núcleo. Pintura com mouse real NÃO foi exercitada (só testes de núcleo e de `ToolState::handle` com eventos sintéticos). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view,paint,fill,geom,blend,compose,layerops,selection,transform,docops,snap,maskops}.rs`, `crates/orvalith/src/{tools,canvas,layers_ui,select,geometry}.rs`, `crates/orvalith/src/app/{io,selops,geocmds}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 14 — vídeo e áudio básicos: verificar bibliotecas locais e licenças (decodificação/codificação, MP4/H.264 só se houver codificador), metadados reais, miniaturas, trilhas, cortes, transições, áudio e exportação com progresso e cancelamento; separar tempo do projeto, índice de quadro e carimbo da mídia. Antes, considerar: APNG/WebP animado (reaproveita `anim.rs`), prévia da animação em thread separada. Pendências: testes manuais com mouse/diálogos (Estágios 2–13); validação cruzada com Photoshop/PhotoCraft; AVIF/HEIC/JXL/RAW; impressão; inpainting; 16 bits/Lab/CMYK.
