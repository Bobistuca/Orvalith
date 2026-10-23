---
type: formato
status: ativo
tags: [orvalith, formato]
---
# Formato de projeto `.orva` (versão 1)

Implementação: [orva.rs](../crates/orvalith-core/src/orva.rs). Decisão: [[docs/decisions/0002-orva-container]].

| Offset | Tamanho | Conteúdo |
|---|---|---|
| 0 | 8 | Magic `ORVALITH` |
| 8 | 4 | Versão do formato (u32 LE) — atual: 1 |
| 12 | 4 | Tamanho do manifesto em bytes (u32 LE) |
| 16 | 4 | CRC32 do manifesto (u32 LE) |
| 20 | N | Manifesto JSON UTF-8 |
| 20+N | — | Blob de recursos |

**Manifesto:** `format` ("orvalith-project"), `app_version`, `name`, `width`, `height`, `ppi` e `layers[]` com `id`, `name`, `visible`, `opacity`, `width`, `height`, `offset`/`stored_len` (no blob), `raw_len` e `crc32` dos pixels descomprimidos.
**Recursos:** cada camada é um fluxo zlib de RGBA8 (sRGB, alfa não pré-multiplicado, linha a linha).

## Evolução
Campos desconhecidos no manifesto são ignorados; mudanças incompatíveis incrementam a versão. Um leitor recusa versões maiores que a sua com mensagem clara. Camadas futuras (grupos, máscaras, texto…) entrarão como novos campos/recursos no manifesto.

## Validação antes de alocar
Magic, versão, tamanho do manifesto (≤ 64 MiB) e CRC; dimensões 1..=32768; no máximo 10 000 camadas; `raw_len == w·h·4`; soma dos pixels ≤ 2 GiB (falha **antes** de alocar); recursos dentro do arquivo; IDs únicos; descompressão limitada a `raw_len + 1` bytes; CRC dos pixels. Teste de "bomba de alocação" em `orva.rs`.

## Limitações da v1
Somente camadas raster planas de 8 bits; sem perfil ICC, máscaras ou metadados de pincel.
