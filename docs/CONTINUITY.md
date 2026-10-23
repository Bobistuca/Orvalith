---
type: continuidade
status: ativo
tags: [orvalith, continuidade]
---
# Continuidade

- **Estágio atual:** 2 concluído como *partial* (ver matriz) → próximo: Estágio 3 (pintura).
- **Funciona:** fmt, clippy -D warnings, 36 testes (núcleo 28 + app 8) e build release OK; app abre/fecha (smoke). Abrir/salvar/exportar via diálogos nativos NÃO foi exercitado manualmente; janela abre e fecha gravando `config.json`; canvas com zoom/pan/abas/status. Interação com mouse não foi exercitada automaticamente.
- **Arquivos-chave:** `crates/orvalith-core/src/{document,compose,history}.rs`, `crates/orvalith/src/{main,app,app/io,canvas,prefs,theme}.rs`, `crates/orvalith-core/src/{orva,imageio,fsio,edit,view}.rs`.
- **Executar:** `cargo run -p orvalith` (PATH: `%USERPROFILE%\.cargo\bin`).
- **Ambiente:** Rust 1.99.1 via rustup; VS Build Tools 2022 instalado via winget (workload C++).
- **Git:** branch `main`, remote `origin` = https://github.com/Bobistuca/Orvalith.git (repositório estava vazio).
- **Integrações:** skill `graphify` instalada em `~/.claude/skills/graphify`; nenhuma skill de Obsidian encontrada (documentação em Markdown com wikilinks). Mapa Graphify ainda não gerado (aguardar mais código).
- **Checks do último checkpoint:** ver [[docs/TESTING]] e CHANGELOG.
- **Bloqueios:** nenhum. Push de main para origin concluído (credencial trocada).
- **Próxima ação:** Estágio 3 — pincel/lápis/borracha (traço contínuo = 1 comando no histórico), conta-gotas, cores, balde, gradiente. Pendências do 2: teste manual de diálogos/autosave/recuperação; mover salvar/abrir para jobs em segundo plano.
