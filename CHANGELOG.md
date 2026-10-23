# Changelog

## Unreleased
- Estágio 4: árvore de camadas com grupos, 25 modos de mesclagem, opacidade/preenchimento, bloqueios, seleção múltipla, mesclar/achatar, Mover, copiar/colar entre documentos, miniaturas, isolamento; pixels das camadas compartilhados (`Arc`) para histórico barato; `.orva` v2.
- Estágio 3: pincel, lápis, borracha, conta-gotas, balde de tinta e gradientes; cores de frente/fundo e amostras; presets de pincel; camada ativa e nova camada; atualização parcial da textura; orçamento de memória do histórico.
- Estágio 2: abrir PNG/JPEG (EXIF), projeto `.orva` v1 versionado e validado, salvar/salvar como atômico, exportar PNG/JPEG, painel de camadas (visibilidade/opacidade) com desfazer/refazer, confirmação ao fechar, autosave/recuperação opcionais, recentes.
- Estágio 1: canvas com zoom/pan/ajustar/100%, abas com fechar e confirmação, atalhos sensíveis a foco, barra de status, preferências locais (tema, escala, janela). Núcleo: `view.rs` com testes.
- Estágio 0: workspace Rust, núcleo (documento, camadas, compositor CPU, histórico), janela eframe com estado vazio e "Novo documento", CI e documentação inicial.
