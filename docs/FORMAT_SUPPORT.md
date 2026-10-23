# Suporte a formatos

| Formato | Leitura | Escrita | Verificado | Limitações |
|---|---|---|---|---|
| `.orva` v1 | sim | sim (atômica) | testes de ida e volta, corrupção, versão futura, bomba de alocação | camadas raster 8 bits; ver [[docs/ORVA_FORMAT]] |
| PNG | sim (8 bits; 16 bits e outros modos são reduzidos a RGBA8 com aviso) | sim (RGBA 8 bits, alfa preservado) | ida e volta exata de pixels | sem perfil ICC, sem metadados, sem PNG animado |
| JPEG (.jpg .jpeg .jpe .jfif) | sim (orientação EXIF aplicada) | sim (qualidade 1–100, composição sobre cor de fundo) | teste de orientação, qualidade e fundo | sem perfil ICC/EXIF na saída |

Demais formatos: Estágio 12. "Supported" só será declarado para o subconjunto verificado. O termo informal "MG" não identifica formato conhecido; nenhum decodificador `.mg` será inventado.
