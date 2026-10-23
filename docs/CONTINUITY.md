---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 12 (12.1 imagens/web/lote, 12.2 PDF, 12.3 PSD/PSB, 12.4 .pcraft leitura, 12.5 evidências) concluído como *partial* → próximo: Estágio 13 (animação e linha do tempo).
- **Funciona:** fmt, clippy -D warnings, 376 testes (núcleo 278 + app 98) e build release OK; app abre/fecha (smoke). Texto/formas/caneta/nós só com eventos sintéticos e testes de núcleo. Pintura com mouse real NÃO foi exercitada (só testes de núcleo e de `ToolState::handle` com eventos sintéticos). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view,paint,fill,geom,blend,compose,layerops,selection,transform,docops,snap,maskops}.rs`, `crates/orvalith/src/{tools,canvas,layers_ui,select,geometry}.rs`, `crates/orvalith/src/app/{io,selops,geocmds}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 13 — linha do tempo (duração, taxa de quadros, cabeça de reprodução), quadro a quadro com cebola, quadros-chave de posição/escala/rotação/opacidade com interpolação, prévia com indicador de taxa, exportação GIF (quantização, transparência, tempo, repetição) e sequência PNG, persistência no `.orva`. Aproveitar GIF/APNG/WebP animados (hoje só o 1º quadro). Pendências: testes manuais com mouse/diálogos (Estágios 2–12); validação cruzada com Photoshop/PhotoCraft reais; escrita de .pcraft; AVIF/HEIC/JXL/RAW; impressão; inpainting; 16 bits/Lab/CMYK.
