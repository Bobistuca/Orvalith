---
type: desempenho
status: ativo
tags: [orvalith, desempenho]
---
# Desempenho (Estágio 16)

Medições feitas em 2026-10-24 neste computador de desenvolvimento (Windows 11, build release, uma única execução cada; **não** são garantias nem comparações com outros programas).

Reprodução: `cargo test -p orvalith-core --locked --release -- --ignored --nocapture perf_`

| Operação | Tempo |
|---|---|
| Compor 4000×3000 com 7 camadas (fundo opaco + 6 com 180/255 de alfa) | ≈ 940 ms |
| Desfoque gaussiano raio 8 em 4000×3000 | ≈ 2,3 s |
| Gravar `.orva` 1920×1080 com 5 camadas | ≈ 20 ms |
| Ler `.orva` 1920×1080 com 5 camadas | ≈ 16 ms |
| Exportar PNG / JPEG / WebP 1920×1080 | ≈ 131 / 305 / 136 ms |

## Medidas já existentes no aplicativo
- Filtros e ajustes pesados rodam em segundo plano com cancelamento e prévia.
- Histórico com orçamento de memória (Preferências), cache de quadros de vídeo limitado, instantâneos com orçamento próprio.
- A composição na tela é feita por região e a textura é reenviada só onde houve alteração.

## Limites conhecidos
- Todo o processamento é na CPU (sem GPU); a composição é de um só núcleo. Documentos muito grandes (dezenas de megapixels com muitas camadas) ficam lentos para compor e filtrar.
- Não houve otimização específica neste estágio: só medição e documentação. Ideias para o futuro: composição paralela por faixas, atalho para camadas opacas no modo Normal, desfoque separável com SIMD.
- Uso de memória por documento não foi medido de forma sistemática.
