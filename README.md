# Orvalith

Editor de imagens desktop escrito em Rust, **sem login, sem rede, armazenamento exclusivamente local**. Interface em português do Brasil. Plataforma principal: Windows 11 x64.

## Estado atual (Estágio 3)
- Janela nativa (eframe/egui) com tema próprio, estado vazio e diálogo "Novo documento".
- Núcleo (`orvalith-core`): documento, camada raster RGBA8, compositor CPU (Normal + opacidade) e histórico desfazer/refazer, com testes.
- Abre PNG/JPEG e projetos `.orva`, salva `.orva` (atômico), exporta PNG/JPEG, painel de camadas (visibilidade/opacidade) com desfazer/refazer, zoom/pan, abas.
- Pinta: pincel, lápis, borracha, conta-gotas, balde de tinta e gradientes, com cores, amostras e presets de pincel; cada traço é uma ação de desfazer.
- Ainda **não** tem seleções, modos de mesclagem, texto, filtros, PDF, PSD etc. Veja [ROADMAP.md](ROADMAP.md) e [docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md).

## Requisitos
- Rust estável (ver `rust-toolchain.toml`) e, no Windows, Visual Studio Build Tools (workload C++).

## Executar
```
cargo run -p orvalith
```

## Documentação
[CLAUDE.md](CLAUDE.md) · [ROADMAP](ROADMAP.md) · [CHANGELOG](CHANGELOG.md) · [Continuidade](docs/CONTINUITY.md) · [Arquitetura](docs/ARCHITECTURE.md) · [Armazenamento local](docs/LOCAL_STORAGE.md) · [Testes](docs/TESTING.md)

Sem screenshots ainda (nenhuma captura original foi feita).
