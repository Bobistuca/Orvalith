---
type: roadmap
status: ativo
tags: [orvalith, roadmap]
---
# Roadmap

Estados: planned · in progress · partial · validated · blocked. Detalhes de cada recurso em [[docs/FEATURE_MATRIX]].

| # | Estágio | Depende de | Estado |
|---|---------|-----------|--------|
| 0 | Inspeção, decisões e setup | — | validated |
| 1 | Aplicativo nativo e área de trabalho (zoom, pan, abas, atalhos) | 0 | partial |
| 2 | Documentos, arquivos (PNG/JPEG, .orva), histórico, autosave opcional | 1 | partial |
| 3 | Pintura, apagar, cores | 2 | partial |
| 4 | Camadas e composição (modos de mesclagem) | 2 | planned |
| 5 | Seleções | 3, 4 | planned |
| 6 | Transformações e geometria | 5 | planned |
| 7 | Máscaras, recorte, canais | 5 | planned |
| 8 | Texto, formas e caminhos | 6, 7 | planned |
| 9 | Ajustes e gerenciamento de cor | 4 | planned |
| 10 | Retoque e filtros clássicos | 5, 9 | planned |
| 11 | Efeitos, recursos editáveis, objetos embutidos | 8, 9 | planned |
| 12 | Imagens, PDF, PSD/PSB, .pcraft | 4, 7, 8 | planned |
| 13 | Animação e linha do tempo | 4 | planned |
| 14 | Vídeo e áudio básicos | 13 | planned |
| 15 | Produtividade | todos | planned |
| 16 | Estabilidade, desempenho, distribuição | todos | planned |

## Checklist do Estágio 0
- [x] Inspeção do ambiente (sem Rust/MSVC; instalados com autorização)
- [x] Repositório Git e remote `origin`
- [x] Workspace Rust (core + app), lockfile, gitignore
- [x] Build, clippy, testes verdes
- [x] Documentação inicial, decisões em [[docs/decisions/0001-stack]]
- [x] CI (`.github/workflows/ci.yml`)
- [ ] Primeiro commit e push

Critérios de conclusão dos demais estágios: ver especificação original (`PROMPT_ORVALITH.md`).
