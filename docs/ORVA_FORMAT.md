---
type: formato
status: ativo
tags: [orvalith, formato]
---
# Formato de projeto `.orva` (versão atual: 2; lê 1 e 2)

Implementação: [orva.rs](../crates/orvalith-core/src/orva.rs). Decisão: [[docs/decisions/0002-orva-container]].

| Offset | Tamanho | Conteúdo |
|---|---|---|
| 0 | 8 | Magic `ORVALITH` |
| 8 | 4 | Versão do formato (u32 LE) — atual: 2 |
| 12 | 4 | Tamanho do manifesto em bytes (u32 LE) |
| 16 | 4 | CRC32 do manifesto (u32 LE) |
| 20 | N | Manifesto JSON UTF-8 |
| 20+N | — | Blob de recursos |

**Manifesto:** `format` ("orvalith-project"), `app_version`, `name`, `width`, `height`, `ppi` e `layers[]` com `id`, `name`, `visible`, `opacity`, `width`, `height`, `offset`/`stored_len` (no blob), `raw_len` e `crc32` dos pixels descomprimidos.
**Recursos:** cada camada é um fluxo zlib de RGBA8 (sRGB, alfa não pré-multiplicado, linha a linha).

## Versão 2 (Estágio 4)
Cada entrada de `layers[]` ganha: `kind` (`raster`/`group`), `parent` (ID do grupo pai), `blend` (snake_case, ex. `linear_dodge`, `pass_through`), `fill`, `lock_pixels`, `lock_alpha`, `lock_position`, `expanded`. Grupos têm `width = height = raw_len = 0` e nenhum recurso. Ausentes em v1 → padrões (raster, raiz, normal, preenchimento 1). Ao ler, a lista é normalizada em pré-ordem (pais desconhecidos ou ciclos viram itens da raiz). A ordem da lista é de baixo para cima.

## Evolução
Campos desconhecidos no manifesto são ignorados; mudanças incompatíveis incrementam a versão. Um leitor recusa versões maiores que a sua com mensagem clara. Camadas futuras (grupos, máscaras, texto…) entrarão como novos campos/recursos no manifesto.

## Validação antes de alocar
Magic, versão, tamanho do manifesto (≤ 64 MiB) e CRC; dimensões 1..=32768; no máximo 10 000 camadas; `raw_len == w·h·4`; soma dos pixels ≤ 2 GiB (falha **antes** de alocar); recursos dentro do arquivo; IDs únicos; descompressão limitada a `raw_len + 1` bytes; CRC dos pixels. Teste de "bomba de alocação" em `orva.rs`.

## Limitações
Camadas raster de 8 bits com o tamanho do documento; sem perfil ICC, máscaras, texto ou metadados de pincel.
