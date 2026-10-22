# Orvalith

Editor de imagens desktop escrito em Rust, **sem login, sem rede, armazenamento exclusivamente local**. Interface em português do Brasil. Plataforma principal: Windows 11 x64.

## Estado atual (Estágio 0)
- Janela nativa (eframe/egui) com tema próprio, estado vazio e diálogo "Novo documento".
- Núcleo (`orvalith-core`): documento, camada raster RGBA8, compositor CPU (Normal + opacidade) e histórico desfazer/refazer, com testes.
- Ainda **não** abre/salva arquivos, não pinta, não tem zoom/pan. Veja [ROADMAP.md](ROADMAP.md) e [docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md).

## Requisitos
- Rust estável (ver `rust-toolchain.toml`) e, no Windows, Visual Studio Build Tools (workload C++).

## Executar
```
cargo run -p orvalith
```

## Documentação
[CLAUDE.md](CLAUDE.md) · [ROADMAP](ROADMAP.md) · [CHANGELOG](CHANGELOG.md) · [Continuidade](docs/CONTINUITY.md) · [Arquitetura](docs/ARCHITECTURE.md) · [Armazenamento local](docs/LOCAL_STORAGE.md) · [Testes](docs/TESTING.md)

Sem screenshots ainda (nenhuma captura original foi feita).
