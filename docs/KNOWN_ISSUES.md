# Problemas conhecidos

- Documentos com lado > 8192 px não são exibidos (textura única, sem tiles); aviso é mostrado no canvas. Resolver com tiles (Estágio 16 ou antes).
- Gradiente e balde guardam no histórico a área alterada inteira (gradiente = camada inteira); em 4K cada ação pode ocupar dezenas de MiB. O orçamento de 256 MiB descarta as ações mais antigas sem avisar.
- Pré-visualização do gradiente recalcula a camada inteira a cada movimento (não medido em documentos grandes).
- Esc só cancela o gradiente; um traço de pincel em andamento não pode ser cancelado (use desfazer).
- Camadas têm sempre o tamanho da tela e as transformações/Mover são destrutivas: pixels que saem da tela são descartados (desfazível, mas perdido após salvar). Deslocamento por camada e objetos inteligentes reversíveis ficam para o Estágio 11.
- Camadas raster têm sempre o tamanho do documento (memória alta com muitas camadas em documentos grandes).
- Reordenar só por botões Subir/Descer; não há arrastar-e-soltar nem mover camadas para dentro/fora de grupos arrastando (use Agrupar/Desagrupar).
- Copiar/colar camadas usa área interna do aplicativo; colar texto/imagem do sistema como camada não existe.
- Operações de seleção (expandir/contrair/suavizar/pena, faixa de cores) rodam na thread da interface; em documentos muito grandes com raios altos podem travar por segundos.
- Cortar não preenche com a cor de fundo e copiar não usa a área de transferência do sistema (apenas a interna do aplicativo).
- A seleção atual não é gravada no `.orva` (só canais salvos).
- Contorno da seleção: documentos com bordas muito complexas desenham no máximo 20 000 trechos por quadro.
- Alças de transformação, recorte, guias, réguas e rotação da visão só foram validados por testes de lógica; nenhuma interação real com mouse foi exercitada.
- A prévia de transformação reprocessa a camada inteira a cada ajuste (bicúbica em 4K pode levar segundos); o trabalho é paralelizado por linhas mas roda na thread da interface.
- Malha de deformação: bordas externas sem suavização; os pontos de controle são fixos em 4×4.
- Réguas desaparecem enquanto a visão está girada; o Mover não encaixa em guias.
- Nenhum outro registrado. Limitações do estágio atual estão em [[docs/FEATURE_MATRIX]].
