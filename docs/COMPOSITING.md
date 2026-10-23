---
type: arquitetura
status: ativo
tags: [orvalith, composicao]
---
# Composição de camadas

Código: [compose.rs](../crates/orvalith-core/src/compose.rs), [blend.rs](../crates/orvalith-core/src/blend.rs), [layerops.rs](../crates/orvalith-core/src/layerops.rs). Matriz: [[docs/FEATURE_MATRIX]].

## Espaço de cor e alfa
- Pixels RGBA8 em sRGB codificado (com gama), alfa **não pré-multiplicado**.
- As fórmulas de mesclagem operam diretamente nos valores codificados (0–1), como os editores de referência fazem por padrão. Não há mesclagem em luz linear nesta fase.
- Composição geral (modos diferentes de Normal), com `αs = alfa da origem × opacidade × preenchimento`:
  `αr = αs + αb(1-αs)` e `Cr = [(1-αs)·αb·Cb + αs·(1-αb)·Cs + αs·αb·B(Cb,Cs)] / αr`.
- Dissolver: o alfa vira probabilidade, comparado a um ruído determinístico por coordenada do documento (a prévia e a exportação coincidem; o padrão de ruído não é o de nenhum outro programa).

## Modos implementados (25 + passar através)
Normal, Dissolver, Escurecer, Multiplicar, Subexposição de cor, Subexposição linear, Clarear, Tela, Superexposição de cor, Superexposição linear (adicionar), Sobrepor, Luz suave (fórmula W3C), Luz direta, Luz brilhante, Luz linear, Luz pontual, Mistura dura (limiar `Cb + Cs ≥ 1`), Diferença, Exclusão, Subtrair, Dividir (`Cs = 0` → 1, exceto `Cb = 0` → 0), Matiz, Saturação, Cor, Luminosidade (fórmulas não separáveis do W3C, luminosidade 0,3/0,59/0,11). "Passar através" existe só para grupos.
Testes: valores conhecidos por modo, identidades (multiplicar por branco etc.), faixa de saída, propriedades dos modos não separáveis, ruído do Dissolver.

## Grupos
- **Passar através** (padrão): os filhos são mesclados diretamente sobre o que está abaixo. Com opacidade < 100% o resultado é interpolado (em alfa pré-multiplicado) entre o fundo e o fundo com o grupo — aproximação do comportamento de referência.
- **Qualquer outro modo** (inclui Normal): o grupo é composto isoladamente sobre transparência e depois mesclado como uma camada única, com o modo, a opacidade e o preenchimento do grupo.
- Grupo oculto oculta os filhos. Grupos aninhados são compostos de dentro para fora.

## Máscaras de camada
- Máscara efetiva = pena (desfoque gaussiano aproximado) e depois densidade (`255 - (255 - m) × densidade`), calculada sob demanda e guardada em cache; só vale se estiver ativa.
- O alfa de origem de cada pixel é multiplicado pela máscara antes de qualquer modo de mesclagem. Os pixels da camada nunca são alterados até **Aplicar**.
- Grupos com máscara são compostos de forma isolada (não "passam através").

## Máscaras de recorte (clipping)
- Uma camada com `clipped` pertence à cadeia da camada não recortada logo abaixo (irmãs). A base é composta sozinha, em modo Normal e sem opacidade; cada camada recortada é mesclada sobre a base com seu modo, opacidade, preenchimento e máscara, com alfa de origem multiplicado pelo alfa da base; o **alfa da base é preservado** (aproximação nos pixels de alfa parcial). O resultado da cadeia é então mesclado com o modo, opacidade e preenchimento da **base**.
- Ocultar a base oculta toda a cadeia; uma camada recortada sem base abaixo se comporta como camada normal. Bases podem ser grupos e podem ter máscara.

## Opacidade × preenchimento
Hoje ambos multiplicam o alfa da camada, então o resultado é idêntico. A diferença (estilos de camada não são afetados pelo preenchimento) só passa a existir no Estágio 11.

## Mesclar e achatar
- *Mesclar selecionadas / visíveis*: os itens são compostos entre si sobre transparência e viram uma camada Normal; modos de mesclagem contra camadas **não** mescladas (abaixo) deixam de valer. Isso difere do "mesclar para baixo" dos editores de referência quando a camada tem modo ≠ Normal.
- *Achatar*: compõe tudo; camadas ocultas são descartadas (com confirmação).

## Diferenças conhecidas e pendências
- Sem máscaras, recorte (clipping) nem estilos de camada (Estágios 7 e 11).
- O mover desloca pixels dentro do buffer da camada (sem deslocamento por camada): o que sai da imagem é descartado.
- Comparação CPU×GPU: **não aplicável** — ainda não existe compositor GPU; o compositor CPU é a referência.
- Sem comparação com resultados de outros programas (não havia um disponível); as fórmulas seguem as especificações públicas citadas.
