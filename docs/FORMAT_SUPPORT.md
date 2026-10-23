# Suporte a formatos

Regra: **"sim" só vale para o subconjunto verificado em testes** (`crates/orvalith-core/src/formats.rs`, `imageio.rs`, `orva.rs`). Reconhecer uma extensão, mostrar miniatura ou renomear um arquivo não é suporte. A tabela abaixo é a mesma que o aplicativo usa (`formats::FORMATS`). Leitura e escrita são avaliadas separadamente.

## Estágio 12.1 — imagens comuns e web

| Família | Extensões | Leitura | Escrita | Verificado por | Limitações conhecidas |
|---|---|---|---|---|---|
| Projeto Orvalith | `.orva` | sim | sim (atômica) | testes de ida e volta, corrupção, versão futura, bomba de alocação | ver [[docs/ORVA_FORMAT]] |
| PNG | `.png` | parcial | parcial | ida e volta exata de pixels e alfa; perfil ICC | 8 bits (16 bits são reduzidos a 8 com aviso); APNG: só o 1º quadro (aviso) |
| JPEG | `.jpg .jpeg .jpe .jfif` | parcial | parcial | orientação EXIF, qualidade, fundo, perfil ICC | sem alfa (composto sobre um fundo); sem EXIF na saída |
| WebP | `.webp` | parcial | parcial | ida e volta exata (sem perdas) com alfa | grava só **sem perdas**; lê com e sem perdas; animação: só o 1º quadro (aviso) |
| GIF | `.gif` | parcial | parcial | gravação e leitura de teste; animado → 1º quadro com aviso | 256 cores e transparência de 1 bit; animação no Estágio 13 |
| BMP | `.bmp` | parcial | parcial | ida e volta exata RGBA | 32 bits; variantes raras do formato não verificadas |
| TIFF | `.tif .tiff` | parcial | parcial | ida e volta exata RGBA 8 bits | uma página, sem compressão na gravação, 8 bits; sem camadas/páginas múltiplas; perfil só se o codec aceitar |
| TGA | `.tga` | parcial | parcial | ida e volta exata RGBA | detectado só pela extensão (não há assinatura) |
| ICO | `.ico` | parcial | parcial | gravação 32×32 e leitura; recusa > 256 px | lê a maior imagem; grava uma imagem |
| QOI | `.qoi` | sim | sim | ida e volta exata RGBA | — |
| PNM | `.pnm .pbm .pgm .ppm` | parcial | parcial | PPM binário | grava PPM sem alfa (composto sobre o fundo) |
| HDR / EXR | `.hdr .exr` | parcial | parcial | ida e volta aproximada (tolerância documentada) | o documento é 8 bits: lê convertendo linear → sRGB 8 bits (valores > 1 cortados, aviso); grava como f32 linear sem alcance extra |
| SVG | `.svg` | parcial | não | rasterização de teste com alfa | importa **rasterizando** (aviso: não fica vetorial); sem exportação vetorial |
| OpenRaster | `.ora` | parcial | parcial | camadas, grupos, visibilidade, opacidade, modos, composição idêntica após reabrir; arquivos hostis | sem máscaras, estilos, ajustes, recortes ou objetos (embutidos no alfa ou omitidos, com aviso) |

Não suportados (visíveis na interface com o motivo):

| Família | Extensões | Motivo |
|---|---|---|
| AVIF | `.avif` | exige decodificador/codificador AV1 externo (dav1d/rav1e) |
| HEIC/HEIF | `.heic .heif` | exige libheif/HEVC (dependência nativa e licenciamento) |
| JPEG XL | `.jxl` | codec de referência nativo e extenso |
| JPEG 2000 | `.jp2` | sem opção leve verificada |
| Camera RAW | `.dng .cr2 .cr3 .nef .arw .raf .orf .rw2` | revelação exige biblioteca por câmera; nenhum modelo foi testado |
| PhotoCraft | `.pcraft` | ver 12.4 abaixo |

## Exportação
Formato, qualidade (JPEG), fundo (JPEG/PNM), lado máximo (miniaturas), perfil de cor (embutir / converter para sRGB / gravar sem perfil), dimensões, avisos de perdas (transparência, camadas, perfil, 8 bits) e **estimativa de tamanho** (codifica na memória). Lote: todos os documentos abertos, cada camada como arquivo, folha de sprites (colunas configuráveis); arquivos existentes nunca são sobrescritos em lote (acrescenta " (n)"). Impressão com dimensões físicas: **não implementada** (a exportação em PDF do 12.2 cobre dimensões físicas).

O termo informal "MG" não identifica formato conhecido; nenhum decodificador `.mg` será inventado.

## Estágio 12.2 — PDF totalmente local

| Recurso | Leitura (importar) | Escrita (exportar) | Verificado por | Limitações |
|---|---|---|---|---|
| PDF | parcial | parcial | `pdf.rs` testes: PDF gerado relido por **leitores independentes** (lopdf conta páginas/MediaBox/imagens; hayro renderiza o conteúdo); PDF protegido (RC4 128 bits criado pelo lopdf) pede senha, recusa senha errada e abre com a correta; recorte, resolução, fundo, limite de tamanho | Leitura: o renderizador `hayro` (Rust puro) cobre a maioria dos PDFs mas não todos (sem blending/isolamento completos, sem recursos não interpretados); páginas viram **imagens raster** (texto/vetores não ficam editáveis); AES e RC4 40 bits não foram testados com arquivos reais; senha só em memória, nunca gravada. Escrita: **somente raster** (uma imagem por página, Flate com máscara de alfa ou JPEG); texto e vetores não são preservados; sem PDF/A, sem marcações |

Importação: miniaturas, escolha de páginas, resolução (ppi), fundo (transparente/branco/cor), recorte em mm, tamanho físico informado. Exportação: vários documentos em um PDF multipágina; tamanho (da imagem, A3, A4, A5, Carta, Ofício, personalizado), orientação, margens, resolução efetiva exibida. Subconjunto "Photoshop PDF": **não suportado** (nenhum dado editável extra é preservado). Tudo local: nenhuma conta, sincronização ou conversão online.

## Subetapas seguintes
12.3 PSD/PSB e 12.4 `.pcraft`: ver as seções abaixo, atualizadas a cada subetapa.
