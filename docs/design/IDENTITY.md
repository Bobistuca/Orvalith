# Identidade visual

- Fundo `#1e2022`, painéis `#26292b` (cinzas neutros).
- Acento turquesa mineral `#2fb8a6`; detalhe cobre `#c97b4a`.
- Interface compacta, legível em 1080p. Ícone original: pendente.

## Tokens da tela inicial (Estágio 17)
Definidos em [theme.rs](../../crates/orvalith/src/theme.rs) (`metrics`, `Palette`); widgets em [kit.rs](../../crates/orvalith/src/kit.rs); ícones em [icons.rs](../../crates/orvalith/src/icons.rs).
- Espaçamento: 4 / 8 / 12 / 16 / 24 px (margem externa 24). Raios: 4 / 6 / 8. Barra lateral: 248 px. Controles: 34 px de altura.
- Tipografia (fonte padrão do egui): título 26, seção 16, corpo 14, apoio 12.
- Escuro (padrão): fundo `#1b1d1f`, superfície `#232628`, elevado `#2b2f32`, hover `#34393d`, borda `#3a3f43`, texto `#e8eaec`, apoio `#a3aaaf`, acento `#2fb8a6`. Claro e alto contraste têm paletas próprias (`Palette::light`, `Palette::high_contrast`).
- Ícones: traço de 1,5 px numa grade de 16 px, desenhados em código (sem arquivos), mesma cor do texto do controle.
- Estados: normal, hover, pressionado, selecionado (barra de acento), desabilitado e foco de teclado (anel de acento de 2 px).
- Código: [theme.rs](../../crates/orvalith/src/theme.rs).
