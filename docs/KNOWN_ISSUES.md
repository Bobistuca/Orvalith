# Problemas conhecidos

- Documentos com lado > 8192 px não são exibidos (textura única, sem tiles); aviso é mostrado no canvas. Resolver com tiles (Estágio 16 ou antes).
- Gradiente e balde guardam no histórico a área alterada inteira (gradiente = camada inteira); em 4K cada ação pode ocupar dezenas de MiB. O orçamento de 256 MiB descarta as ações mais antigas sem avisar.
- Pré-visualização do gradiente recalcula a camada inteira a cada movimento (não medido em documentos grandes).
- Esc só cancela o gradiente; um traço de pincel em andamento não pode ser cancelado (use desfazer).
- Nenhum outro registrado. Limitações do estágio atual estão em [[docs/FEATURE_MATRIX]].
