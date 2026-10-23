---
type: formato
status: ativo
tags: [orvalith, formato]
---
# Formato de projeto `.orva` (versão atual: 5; lê 1 a 5)

Implementação: [orva.rs](../crates/orvalith-core/src/orva.rs). Decisão: [[docs/decisions/0002-orva-container]].

| Offset | Tamanho | Conteúdo |
|---|---|---|
| 0 | 8 | Magic `ORVALITH` |
| 8 | 4 | Versão do formato (u32 LE) — atual: 5 |
| 12 | 4 | Tamanho do manifesto em bytes (u32 LE) |
| 16 | 4 | CRC32 do manifesto (u32 LE) |
| 20 | N | Manifesto JSON UTF-8 |
| 20+N | — | Blob de recursos |

**Manifesto:** `format` ("orvalith-project"), `app_version`, `name`, `width`, `height`, `ppi` e `layers[]` com `id`, `name`, `visible`, `opacity`, `width`, `height`, `offset`/`stored_len` (no blob), `raw_len` e `crc32` dos pixels descomprimidos.
**Recursos:** cada camada é um fluxo zlib de RGBA8 (sRGB, alfa não pré-multiplicado, linha a linha).

## Versão 2 (Estágio 4)
Cada entrada de `layers[]` ganha: `kind` (`raster`/`group`), `parent` (ID do grupo pai), `blend` (snake_case, ex. `linear_dodge`, `pass_through`), `fill`, `lock_pixels`, `lock_alpha`, `lock_position`, `expanded`. Grupos têm `width = height = raw_len = 0` e nenhum recurso. Ausentes em v1 → padrões (raster, raiz, normal, preenchimento 1). Ao ler, a lista é normalizada em pré-ordem (pais desconhecidos ou ciclos viram itens da raiz). A ordem da lista é de baixo para cima.

## Versão 3 (Estágio 5)
`channels[]` no manifesto: canais alfa auxiliares (seleções salvas) com `id`, `name`, `offset`/`stored_len`, `raw_len` (= largura × altura do documento) e `crc32`; cada canal é um fluxo zlib de 1 byte por pixel (255 = selecionado). Validados como as camadas (tamanho exato, limites, CRC, no máximo 256 canais, contam para o limite de 2 GiB). A seleção atual **não** é gravada.

## Versão 4 (Estágio 6)
`guides[]` no manifesto: guias de régua (`vertical`, `pos` em pixels do documento, finito). Valores inválidos são descartados na leitura (máx. 10 000).

## Versão 5 (Estágio 7)
Cada camada ganha `clipped` (recortada na camada abaixo) e `mask` opcional: `offset`/`stored_len`/`raw_len`/`crc32` de um recurso zlib de 1 byte por pixel do documento, mais `enabled`, `linked`, `density` (0..1) e `feather` (px). `raw_len` deve ser `largura × altura` do documento; a leitura valida tamanho, limites, CRC e conta a memória no limite de 2 GiB. Máscaras de grupo são aceitas.

## Versão 6 (Estágio 8)
Cada camada ganha `source` opcional (`{"Text": …}` ou `{"Shape": …}`: especificação editável de texto ou forma/caminho) e cada máscara ganha `vector` opcional (forma que a gerou). Os pixels continuam gravados, então a aparência independe de fontes instaladas. Leitura leniente: `source`/`vector` desconhecidos, inválidos ou fora dos limites (`source.rs`) são ignorados e a camada vira raster, sem erro. Fontes ausentes geram aviso ao abrir.

## Versão 7 (Estágio 9)
Cada camada ganha `adjustment` opcional (JSON do ajuste: tipo e parâmetros; camada transparente do tamanho do documento). O manifesto ganha `icc` opcional (`name`, `offset`, `stored_len`, `raw_len`, `crc32` de um recurso zlib com os bytes do perfil, ≤ 16 MiB). Leitura leniente: ajuste desconhecido ou com parâmetros fora dos limites é ignorado (camada transparente comum); os bytes do perfil são preservados mesmo que este leitor não os entenda, e o CRC é validado.

## Versão 8 (Estágio 11.1)
Cada camada ganha `styles` opcional (lista JSON de efeitos: tipo, ativo e parâmetros) e `source` passa a aceitar `Fill` (cor, gradiente ou padrão). Leitura leniente: estilos fora dos limites (`style::effects_sane`) são ignorados e a camada fica sem estilos; os pixels gravados continuam valendo.

## Versão 9 (Estágio 11.2)
Cada camada ganha `object` opcional: `orig_w`/`orig_h`, `cx`/`cy`/`sx`/`sy`/`rot`, `link` (caminho), `data` (original RGBA8 como recurso zlib; só em objetos **embutidos**), `filters` (lista JSON lida de forma tolerante, máx. 16) e `mask` (recurso zlib do tamanho do documento). Validação antes de alocar: lados 1..=32768, `raw_len == orig_w·orig_h·4`, soma no limite de 2 GiB, recursos dentro do arquivo, CRC. Vínculos de rede/URL (`\\`, `//`, `://`) são descartados; sem original nem vínculo válido o objeto é ignorado (a camada fica raster com os pixels gravados). Objetos vinculados são relidos do disco ao abrir (`object::resolve_links`); arquivo ausente gera aviso e a camada mantém a aparência gravada.

## Versão 10 (Estágio 11.3)
O manifesto ganha `comps`: lista (máx. 64) de composições `{name, states:[{layer, visible, opacity, fill, blend, styles:[bool]}]}` lida de forma tolerante; entradas inválidas ou listas fora dos limites são descartadas.

## Evolução
Campos desconhecidos no manifesto são ignorados; mudanças incompatíveis incrementam a versão. Um leitor recusa versões maiores que a sua com mensagem clara. Camadas futuras (grupos, máscaras, texto…) entrarão como novos campos/recursos no manifesto.

## Validação antes de alocar
Magic, versão, tamanho do manifesto (≤ 64 MiB) e CRC; dimensões 1..=32768; no máximo 10 000 camadas; `raw_len == w·h·4`; soma dos pixels ≤ 2 GiB (falha **antes** de alocar); recursos dentro do arquivo; IDs únicos; descompressão limitada a `raw_len + 1` bytes; CRC dos pixels. Teste de "bomba de alocação" em `orva.rs`.

## Limitações
Camadas raster de 8 bits com o tamanho do documento; sem perfil ICC ou metadados de pincel; texto/forma guardam só a origem e os pixels.
