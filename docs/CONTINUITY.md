---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 15.1 (comandos, atalhos, paleta, preferências) concluída como *partial*; faltam 15.2 (ações/macros e lote) e 15.3 (predefinições, importação GPL/GGR, espaços de trabalho, instantâneos, ajuda e exemplos).
- **Funciona:** fmt, clippy -D warnings, 404 testes (núcleo 298 + app 106) e build release OK; app abre/fecha (smoke). Texto/formas/caneta/nós só com eventos sintéticos e testes de núcleo. Pintura com mouse real NÃO foi exercitada (só testes de núcleo e de `ToolState::handle` com eventos sintéticos). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view,paint,fill,geom,blend,compose,layerops,selection,transform,docops,snap,maskops}.rs`, `crates/orvalith/src/{tools,canvas,layers_ui,select,geometry}.rs`, `crates/orvalith/src/app/{io,selops,geocmds}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 15 — atalhos personalizáveis e busca de comandos, espaços de trabalho e layouts persistentes, gravação/reprodução de ações (macros) sobre o motor de comandos, processamento em lote com relatório por arquivo (sem sobrescrever originais sem opção explícita), predefinições, histórico visual e instantâneos dentro do orçamento, importação de recursos (pincéis) só de formatos realmente suportados, preferências de memória/cache/autosave, ajuda local e exemplos originais, acessibilidade. Pendências: testes manuais com mouse/diálogos (Estágios 2–14); players externos para o vídeo; validação cruzada com Photoshop/PhotoCraft; AVIF/HEIC/JXL/RAW; impressão; inpainting; 16 bits/Lab/CMYK; reprodução de áudio no aplicativo.
