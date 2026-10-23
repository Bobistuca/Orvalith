# Testes

Comandos em [[CLAUDE]] e [[docs/COMMIT_CHECKLIST]].

- Automatizados: `cargo test --workspace --locked` (núcleo: composição alfa, histórico, validação de dimensões).
- Manual (UI): `cargo run -p orvalith` → "Novo documento" → criar 1920×1080 → ver xadrez/fundo branco; Ctrl+N abre o diálogo.
- Última execução registrada: ver [[docs/CONTINUITY]].

## Estágio 7 (2026-10-23)
fmt, clippy, 219 testes e build release OK; smoke test abre/fecha. Manual: painel Camadas → "+ Ocultar tudo" cria máscara; selecione "Máscara" em "Pintar em" (ou clique na miniatura da máscara), pinte de branco com B para revelar e de preto para ocultar; Ctrl+Z desfaz por traço; Alt+clique na miniatura da máscara mostra-a em cinza; densidade/pena; Aplicar/Excluir/Inverter/→ Seleção/Refinar…; Ctrl+Alt+G recorta na camada abaixo (veja a cadeia); mover a camada com a máscara vinculada e desvinculada; Canais: ver R/G/B, salvar seleção como canal, Editar (máscara rápida), Selecionar, renomear (duplo clique), excluir; salvar `.orva` e reabrir.

## Estágio 6 (2026-10-23)
fmt, clippy, 192 testes e build release OK; smoke test abre/fecha. Manual: Ctrl+T → arrastar dentro (mover), canto (escalar; Shift mantém proporção), fora (girar; Shift = passos de 15°), pivô e valores numéricos na barra, Enter aplica/Esc cancela (1 ação); Imagem → Transformar camada (inclinar, distorcer, perspectiva, malha 4×4, espelhar, girar); C recorta (alças; Enter); Imagem → Tamanho da imagem/da tela/Endireitar/Girar tela; Camada→Alinhar; arrastar das réguas cria guias (arrastar de volta remove); Ctrl+R/Ctrl+'/Ctrl+;/Ctrl+Shift+; alternam réguas/grade/guias/encaixe; Alt+←/→ gira a visão, Alt+↑ redefine; salvar `.orva` e reabrir conferindo dimensões e guias.

## Estágio 5 (2026-10-23)
fmt, clippy, 149 testes e build release OK; smoke test abre/fecha. Manual: M arrasta um retângulo (formigas animadas); Shift+arrastar adiciona, Alt subtrai; arrastar de dentro move a seleção; L laço livre; Shift+L polígono (duplo clique/Enter fecha, Esc cancela); W varinha; Selecionar → Faixa de cores (prévia ao vivo) / Modificar / Transformar / Canais; com seleção: pincel, balde e gradiente não saem dela, Delete apaga, Alt+Backspace preenche com a cor de frente, Ctrl+X/C e Ctrl+V entre documentos; Q entra na máscara rápida (pinte, Q sai); Ctrl+Z desfaz seleção e edições; salvar o `.orva` com um canal e reabrir.

## Estágio 4 (2026-10-23)
fmt, clippy, 112 testes e build release OK; smoke test abre/fecha. Manual: duas camadas com cores diferentes → trocar o modo (Multiplicar, Tela, Luminosidade…) e conferir a mudança; Ctrl+clique para selecionar várias → Ctrl+G agrupa, Ctrl+Shift+G desagrupa, Ctrl+E mescla; Alt+clique no olho isola; duplo clique renomeia; V move (Esc cancela); Ctrl+C num documento e Ctrl+V em outro; salvar `.orva`, reabrir e conferir grupos/modos/bloqueios; Camada → Achatar pede confirmação.

## Estágio 3 (2026-10-23)
fmt, clippy, 70 testes e build release OK; smoke test abre/fecha. Manual: Ctrl+N → B (pincel): arrastar rápido em círculos, conferir ausência de lacunas e que Ctrl+Z desfaz o traço inteiro; [ e ] mudam o tamanho; E apaga; I amostra (Alt = fundo); G balde (Tolerância/Contíguo); Shift+G gradiente (Esc cancela durante o arrasto); X troca cores, D restaura; salvar preset e reiniciar o app; salvar `.orva`, reabrir e comparar a pintura; testar em zoom 25%, 100% e 800%.

## Estágio 9 (2026-10-23)
fmt, clippy -D warnings, 284 testes (núcleo 201 + app 83) e build release OK; smoke test: abre e fecha. Manual: abrir uma imagem; Ajustes → Níveis… (arrastar, Aplicar, Ctrl+Z); Ajustes → Nova camada de ajuste → Curvas (arrastar pontos no painel da camada; ocultar a camada), salvar `.orva`, reabrir e editar de novo; Ajustes → Histograma; abrir PNG/JPEG com perfil Display P3/Adobe RGB (aviso), Ajustes → Perfil de cor (atribuir x converter), exportar com cada opção de perfil.

## Estágio 8 (2026-10-23)
fmt, clippy -D warnings, 253 testes (núcleo 175 + app 78) e build release OK; smoke test: abre e fecha. Testes de texto pulam se não houver fontes do sistema. Manual: T → clicar → digitar com acentos → Ctrl+Enter; U → arrastar forma; P → clicar/arrastar para curvas, Enter; A → mover nós; selecionar 2 formas (Ctrl+clique) → Texto e formas → Combinar; salvar `.orva`, reabrir e editar de novo.

## Estágio 2 (2026-10-23)
fmt, clippy, 36 testes e build release OK; smoke test: abre e fecha sem criar pasta de recuperação (autosave desligado). Manual: Ctrl+N → painel Camadas: marcar/desmarcar visibilidade e arrastar opacidade → título ganha `*`, Ctrl+Z/Ctrl+Shift+Z; Ctrl+S → escolher `.orva` → fechar e reabrir com Ctrl+O; Ctrl+Shift+E → PNG/JPEG; arrastar um PNG/JPEG para a janela; ativar autosave (Editar → Preferências), editar, encerrar o processo à força e reabrir para ver a recuperação.

## Estágio 1 (2026-10-22)
fmt, clippy, 16 testes, build release OK; smoke test: abre, fecha, grava preferências. Procedimento manual: Ctrl+N ×2 → abas; roda do mouse = zoom no cursor; Espaço+arrastar ou botão do meio = pan; Ctrl+0/Ctrl+1; × fecha aba; Editar → Preferências (escala/tema) e reabrir o app.

## Estágio 0 (2026-10-22)
fmt, clippy -D warnings, 8 testes do núcleo e build release OK; executável release abre a janela "Orvalith" (verificado por processo/título; sem captura de tela nem teste de interação). Teste com rede bloqueada: pendente.
