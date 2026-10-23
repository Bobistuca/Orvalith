---
type: decisao
status: aceita
tags: [orvalith, decisao, formato]
---
# 0002 — Contêiner `.orva` próprio (manifesto JSON + recursos zlib)

- **Escolha:** cabeçalho binário + manifesto JSON + blob de recursos comprimidos (flate2/zlib) com CRC32 (crc32fast), em vez de ZIP.
- **Por quê:** validação de tamanhos antes de alocar totalmente sob nosso controle; poucas dependências; fácil de versionar.
- **Trade-offs:** não abre em ferramentas ZIP; sem leitura parcial/tiles ainda (todo o projeto é lido para a memória). Reavaliar para tiles no Estágio 16.
- Especificação: [[docs/ORVA_FORMAT]]. Escrita sempre atômica ([fsio.rs](../../crates/orvalith-core/src/fsio.rs)).
