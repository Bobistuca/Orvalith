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

## Estágio 15.3 (2026-10-24)
fmt, clippy -D warnings, 422 testes (núcleo 308 + app 114) e build release OK; smoke test: abre e permanece em execução. Manual: F1 (ajuda, buscar "gimp", abrir exemplo Retoque); Editar → Predefinições…; Novo documento → Predefinição; Exportar → Salvar predefinição; Visualizar → Espaço de trabalho → Mínimo e reabrir o app; Editar → Histórico e instantâneos…: criar, editar, restaurar; Predefinições → Importar paleta/gradiente GIMP.

## Estágio 15.2 (2026-10-23)
fmt, clippy -D warnings, 408 testes (núcleo 301 + app 107) e build release OK; smoke test: abre e fecha. Manual: Editar → Ações → Gravar; aplicar Inverter e Reduzir; Parar, nomear e Salvar; Reproduzir em outro documento; Lote…: escolher arquivos (incluindo um corrompido e um .xyz), pasta de saída, Processar e conferir o relatório; repetir sem a opção de sobrescrever (arquivos "(2)").

## Estágio 15.1 (2026-10-23)
fmt, clippy -D warnings, 404 testes (núcleo 298 + app 106) e build release OK; smoke test: abre e fecha. Manual: Ctrl+Shift+P e digitar "mediana"; Editar → Atalhos de teclado… → Definir… num comando, conferir conflito e Restaurar; reabrir o aplicativo e ver o atalho mantido; Preferências → Alto contraste.

## Estágio 14 (2026-10-23)
fmt, clippy -D warnings, 399 testes (núcleo 298 + app 101) e build release OK; smoke test: abre e fecha. Os testes de mídia usam o FFmpeg real (FFmpeg 9.0.1 gyan.dev full) e são pulados se ele não existir; o de aceitação monta 2 clipes (vermelho/azul com áudio), apara, adiciona título, dissolve, exporta e confere com o ffprobe (dimensões, taxa, duração, áudio) e re-extração de quadros. Manual: Animação → Vídeo… → Adicionar mídia…; arrastar bordas e corpo dos clipes; Documento atual como título; Dissolver; Exportar; abrir o MP4 em outro player (pendente); salvar `.orva`, mover a pasta e reabrir; apagar a mídia e usar Localizar….

## Estágio 13 (2026-10-23)
fmt, clippy -D warnings, 386 testes (núcleo 287 + app 99) e build release OK; smoke test: abre e fecha. Manual: Animação → Linha do tempo → Criar animação; selecionar uma camada, no quadro 0 adicionar chave de Posição X, ir ao quadro 24 e mudar X; reproduzir; criar quadros ("Novo quadro") com camadas ligadas/desligadas; ligar a pele de cebola; Exportar GIF e abrir no navegador (repetição e tempo); Exportar sequência PNG; salvar `.orva` e reabrir.

## Estágio 12.3–12.5 (2026-10-23)
fmt, clippy -D warnings, 376 testes (núcleo 278 + app 98) e build release OK; smoke test: abre e fecha. Manual: exportar um documento com grupos/máscaras como .psd e .psb e abrir em outro editor (pendente de Photoshop); abrir um .psd real; abrir um .pcraft real do PhotoCraft (pendente). Os testes de PDF levam ~20 s em modo debug (renderizador).

## Estágio 12.2 (2026-10-23)
fmt, clippy -D warnings, 368 testes (núcleo 268 + app 98) e build release OK; smoke test: abre e fecha. Manual: Arquivo → Exportar PDF… (A4, vários documentos) e abrir o arquivo em um leitor de PDF externo; Arquivo → Importar PDF… com um PDF de várias páginas e um PDF com senha.

## Estágio 12.1 (2026-10-23)
fmt, clippy -D warnings, 360 testes (núcleo 262 + app 98) e build release OK. Manual: Arquivo → Abrir um .webp/.gif/.bmp/.tif/.svg/.ora; Exportar: escolher cada formato, ver avisos e "Estimar tamanho"; lote para uma pasta (documentos, camadas, sprites). Sem rede: os formatos usam só bibliotecas locais.

## Estágio 11.3 (2026-10-23)
fmt, clippy -D warnings, 351 testes (núcleo 253 + app 98) e build release OK; smoke test: abre e fecha. Manual: Objetos → Composições de camadas…; criar "A", esconder uma camada, criar "B", alternar clicando; ● marca a composição ativa; salvar `.orva` e reabrir.

## Estágio 11.2 (2026-10-23)
fmt, clippy -D warnings, 346 testes (núcleo 249 + app 97) e build release OK; smoke test: abre e fecha. Manual: Objetos → Converter camada em objeto inteligente; no painel mudar escala para 25% e voltar para 100% (a imagem deve voltar nítida); adicionar Filtro inteligente (Pixelar), desligar/ligar; fazer uma seleção e "Máscara dos filtros: da seleção"; Objetos → Colocar imagem vinculada…, salvar `.orva`, renomear/mover o arquivo original e reabrir (aviso de vínculo ausente), Reencontrar…; Embutir.

## Estágio 11.1 (2026-10-23)
fmt, clippy -D warnings, 334 testes (núcleo 238 + app 96) e build release OK; smoke test: abre e fecha. Manual: criar texto/forma, abrir o painel da camada → Estilos → adicionar Sombra projetada, Traço e Sobreposição de gradiente; ativar/desativar/reordenar; salvar predefinição e aplicar em outra camada; Preenchimento → Nova camada: Gradiente; salvar `.orva`, reabrir e editar.

## Estágio 10.2 (2026-10-23)
fmt, clippy -D warnings, 318 testes (núcleo 225 + app 93) e build release OK; smoke test: abre e fecha. Manual: Shift+R; arrastar sobre um rosto/objeto (Deformar); Suavizar e Reconstruir por cima; Esc cancela; Enter aplica e Ctrl+Z desfaz; com seleção ativa só a seleção deforma.

## Estágio 10.1 (2026-10-23)
fmt, clippy -D warnings, 309 testes (núcleo 219 + app 90) e build release OK; smoke test: abre e fecha. Manual: abrir uma foto; Filtro → Desfoque gaussiano… (arrastar o raio; "Calculando…"; Cancelar deve restaurar; Aplicar + Ctrl+Z); fazer uma seleção e repetir; S: Alt+clique numa origem e pintar; marcar "Correção"; J numa mancha; selecionar e H arrastando até uma área boa; Y num olho vermelho; R com cada pincel.

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
