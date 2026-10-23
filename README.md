# Orvalith

Editor de imagens desktop escrito em Rust, **sem login, sem rede, armazenamento exclusivamente local**. Interface em português do Brasil. Plataforma principal: Windows 11 x64.

## Estado atual (Estágio 10.1)
- Janela nativa (eframe/egui) com tema próprio, estado vazio e diálogo "Novo documento".
- Núcleo (`orvalith-core`): documento, camada raster RGBA8, compositor CPU (Normal + opacidade) e histórico desfazer/refazer, com testes.
- Abre PNG/JPEG e projetos `.orva`, salva `.orva` (atômico), exporta PNG/JPEG, painel de camadas (visibilidade/opacidade) com desfazer/refazer, zoom/pan, abas.
- Pinta: pincel, lápis, borracha, conta-gotas, balde de tinta e gradientes, com cores, amostras e presets de pincel; cada traço é uma ação de desfazer.
- Camadas: grupos aninhados, 25 modos de mesclagem, opacidade/preenchimento, bloqueios, seleção múltipla, mesclar/achatar, mover, copiar/colar entre documentos, miniaturas.
- Seleções: retângulo, elipse, laço, polígono, varinha, faixa de cores, modos de combinação, modificar, pena, canais salvos, máscara rápida; cortar/copiar/preencher e ferramentas respeitam a seleção.
- Geometria: transformação livre/inclinar/distorcer/perspectiva/malha, espelhar e girar, alinhar, recorte, endireitar, tamanho da imagem e da tela, réguas, guias, grade, encaixe e rotação da visão.
- Máscaras de camada (pintáveis, vinculadas, com densidade e pena), máscaras de recorte e painel de canais.
- Texto editável (T), formas (U), caneta (P) e seleção de nós (A); booleanas, caminho → seleção/máscara vetorial e rasterização (menu "Texto e formas").
- Menu Ajustes: 16 ajustes (níveis, curvas, matiz/saturação…) destrutivos com prévia ou como camadas de ajuste editáveis, histograma e perfil de cor (ICC).
- Filtro: 14 filtros clássicos com prévia em segundo plano; retoque: carimbo (S), correção pontual (J), remendo (H), olhos vermelhos (Y) e pincéis de efeito (R).
- Ainda **não** tem liquify, estilos de camada, 16 bits, PDF, PSD etc. Veja [ROADMAP.md](ROADMAP.md) e [docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md).

## Requisitos
- Rust estável (ver `rust-toolchain.toml`) e, no Windows, Visual Studio Build Tools (workload C++).

## Executar
```
cargo run -p orvalith
```

## Documentação
[CLAUDE.md](CLAUDE.md) · [ROADMAP](ROADMAP.md) · [CHANGELOG](CHANGELOG.md) · [Continuidade](docs/CONTINUITY.md) · [Arquitetura](docs/ARCHITECTURE.md) · [Armazenamento local](docs/LOCAL_STORAGE.md) · [Testes](docs/TESTING.md)

Sem screenshots ainda (nenhuma captura original foi feita).
