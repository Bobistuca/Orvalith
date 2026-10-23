# Problemas conhecidos

- Documentos com lado > 8192 px não são exibidos (textura única, sem tiles); aviso é mostrado no canvas. Resolver com tiles (Estágio 16 ou antes).
- Gradiente e balde guardam no histórico a área alterada inteira (gradiente = camada inteira); em 4K cada ação pode ocupar dezenas de MiB. O orçamento de 256 MiB descarta as ações mais antigas sem avisar.
- Pré-visualização do gradiente recalcula a camada inteira a cada movimento (não medido em documentos grandes).
- Esc só cancela o gradiente; um traço de pincel em andamento não pode ser cancelado (use desfazer).
- Ferramenta Mover desloca pixels no buffer da camada (sem deslocamento por camada): o que sai da imagem é descartado (desfazível, mas perdido após salvar). Deslocamento por camada virá com transformações (Estágio 6).
- Camadas raster têm sempre o tamanho do documento (memória alta com muitas camadas em documentos grandes).
- Reordenar só por botões Subir/Descer; não há arrastar-e-soltar nem mover camadas para dentro/fora de grupos arrastando (use Agrupar/Desagrupar).
- Copiar/colar camadas usa área interna do aplicativo; colar texto/imagem do sistema como camada não existe.
- Nenhum outro registrado. Limitações do estágio atual estão em [[docs/FEATURE_MATRIX]].
