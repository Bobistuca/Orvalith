---
type: arquitetura
status: ativo
tags: [orvalith, arquitetura]
---
# Arquitetura

Decisão base: [[docs/decisions/0001-stack]].

## Crates
- `orvalith-core`: [document.rs](../crates/orvalith-core/src/document.rs) (Document, Layer, IDs, revisão), [compose.rs](../crates/orvalith-core/src/compose.rs) (compositor CPU de referência), [history.rs](../crates/orvalith-core/src/history.rs) (comandos reversíveis).
- `orvalith` (app): [app.rs](../crates/orvalith/src/app.rs) UI, [theme.rs](../crates/orvalith/src/theme.rs) identidade visual.

## Convenções
- Coordenadas em pixels do documento, independentes de zoom/DPI.
- Pixels RGBA8, sRGB, alfa **não pré-multiplicado**; conversões nas bordas de import/export.
- `Document::revision` avança a cada edição para descartar resultados assíncronos obsoletos.
- Histórico por documento; cada `Command` é uma ação atômica (um traço = um comando).
- Precisão interna futura: evoluir para 16 bits/float sem converter silenciosamente para 8 bits (pendente).

## Arquivos (Estágio 2)
[orva.rs](../crates/orvalith-core/src/orva.rs) (formato, [[docs/ORVA_FORMAT]]), [imageio.rs](../crates/orvalith-core/src/imageio.rs) (PNG/JPEG), [fsio.rs](../crates/orvalith-core/src/fsio.rs) (escrita atômica), [edit.rs](../crates/orvalith-core/src/edit.rs) (comandos de camada). `History::record` registra uma ação já aplicada (arrasto de controle = uma ação). UI de arquivos em [io.rs](../crates/orvalith/src/app/io.rs).

## Pintura (Estágio 3)
[paint.rs](../crates/orvalith-core/src/paint.rs): `StrokeSession` carimba discos espaçados ao longo do caminho; cobertura do traço em buffer próprio (fluxo por carimbo) e opacidade aplicada uma vez sobre o original da camada; no fim vira um `PixelPatch` (uma ação). [fill.rs](../crates/orvalith-core/src/fill.rs): balde (varredura por linhas) e gradientes. [tools.rs](../crates/orvalith/src/tools.rs) interpreta a entrada; [canvas.rs](../crates/orvalith/src/canvas.rs) devolve eventos de ponteiro (todas as amostras do quadro, em coordenadas do documento) e atualiza a textura só na região alterada (`mark_dirty`/`record_patch`).

## Camadas (Estágio 4)
`Layer.pixels` é `Arc<Vec<u8>>` (copy-on-write via `pixels_mut`): clonar a lista de camadas é barato, e `edit::LayersEdit` guarda antes/depois da lista inteira — operações estruturais (agrupar, mesclar, mover, renomear, bloqueios…) são uma ação cada, e só buffers novos contam para o orçamento do histórico. [layerops.rs](../crates/orvalith-core/src/layerops.rs) são funções puras que devolvem a nova lista (invariante: pré-ordem, `normalize`). Detalhes de composição em [[docs/COMPOSITING]]. UI: [layers_ui.rs](../crates/orvalith/src/layers_ui.rs).

## Seleções (Estágio 5)
[selection.rs](../crates/orvalith-core/src/selection.rs): máscaras de 8 bits (255 = selecionado) com formas (retângulo, elipse, polígono), varinha, faixa de cores, morfologia (disco), pena, suavização, transformação e as operações `clear/fill/extract_selected`. O documento guarda `selection` e `channels` (com `Arc`); alterações passam por `edit::SelectionEdit` (histórico, sem sujar o documento nem reenviar a textura). Pincel, balde e gradiente recebem a máscara e misturam o resultado pixel a pixel. UI: [select.rs](../crates/orvalith/src/select.rs) (ferramentas e diálogos) e [selops.rs](../crates/orvalith/src/app/selops.rs) (cortar/copiar/preencher/máscara rápida). A máscara rápida é um documento cinza próprio, editado pelas mesmas ferramentas, com histórico separado até sair.

## Geometria (Estágio 6)
[transform.rs](../crates/orvalith-core/src/transform.rs): `Affine`, `Homography`, amostragem (vizinho/bilinear/bicúbica) em alfa pré-multiplicado, `warp*` (paralelo por linhas) e `resample` separável com filtro alargado ao reduzir. [docops.rs](../crates/orvalith-core/src/docops.rs): `DocState`/`DocEdit` (estado geométrico inteiro, pixels por `Arc`), recorte, tela, imagem, orientação, endireitar, alinhar/distribuir. [snap.rs](../crates/orvalith-core/src/snap.rs) e rotação em [view.rs](../crates/orvalith-core/src/view.rs). UI: [geometry.rs](../crates/orvalith/src/geometry.rs) (sessão de transformação com prévia viva, ferramenta de recorte, diálogos) e [canvas.rs](../crates/orvalith/src/canvas.rs) (réguas, guias, grade, malha de textura girável).

## Máscaras e canais (Estágio 7)
`Layer.mask: Option<LayerMask>` ([document.rs](../crates/orvalith-core/src/document.rs)) com cache da máscara efetiva; [maskops.rs](../crates/orvalith-core/src/maskops.rs) (criar/aplicar/inverter/refinar, funções puras) e `edit::MaskPatch` (pintura na máscara com desfazer). A pintura na máscara reaproveita o motor de traço: uma camada-sombra cinza recebe as ferramentas e cada etapa é copiada de volta para a máscara real (prévia ao vivo); o fim do gesto vira um `MaskPatch`. A composição (`compose.rs`) trata cadeias de recorte e máscaras. O painel de canais usa a máscara rápida (`qm_target`) para editar canais salvos.

## Texto, formas e caminhos (Estágio 8)
[vector.rs](../crates/orvalith-core/src/vector.rs) (formas, caminhos Bézier, cobertura e renderização com `tiny-skia`, booleanas), [text.rs](../crates/orvalith-core/src/text.rs) (`cosmic-text`: fontes do sistema, modelagem, fallback), [source.rs](../crates/orvalith-core/src/source.rs) (`LayerSource` + limites) e [vecops.rs](../crates/orvalith-core/src/vecops.rs) (criar/atualizar/rasterizar/combinar, máscara vetorial). A camada continua do tamanho da tela com pixels renderizados; `Layer.source` guarda a especificação editável e `rerender` regera os pixels. Operações que não preservam a origem pedem rasterização. UI: [vectool.rs](../crates/orvalith/src/vectool.rs).

## Ajustes e cor (Estágio 9)
[adjust.rs](../crates/orvalith-core/src/adjust.rs) (`Adjustment`, `Prepared` com LUT para ajustes por canal), [adjops.rs](../crates/orvalith-core/src/adjops.rs) (camada de ajuste, aplicação destrutiva com seleção), [histogram.rs](../crates/orvalith-core/src/histogram.rs) e [color.rs](../crates/orvalith-core/src/color.rs) (`moxcms`: inspeção, conversão RGBA8, `Preview`, `ColorEdit`). `Layer.adjustment` marca camadas de ajuste; `compose.rs` aplica o ajuste ao que já está no buffer (alfa intacto) com opacidade, modo, máscara e recorte (no recorte age só sobre a base). `Document.icc` é o perfil atribuído (`None` = sRGB). UI: [adjustui.rs](../crates/orvalith/src/adjustui.rs) (editor compartilhado, sessão destrutiva com prévia, histograma, diálogo de perfil); a prévia de cor converte a região composta para sRGB em `render_composite`.

## Filtros e retoque (Estágio 10.1)
[filter.rs](../crates/orvalith-core/src/filter.rs) (`Filter`, `apply`/`apply_selected` com `AtomicBool` de cancelamento; desfoques e mediana em alfa pré-multiplicado) e [retouch.rs](../crates/orvalith-core/src/retouch.rs) (toques de clone/correção, correção pontual, remendo, olhos vermelhos, pincéis de efeito, `Walker` de espaçamento). UI: [filterui.rs](../crates/orvalith/src/filterui.rs) (sessão: thread que calcula a prévia, reinicia ao mudar parâmetros, descarta resultados cancelados; Aplicar só quando o resultado bate com os parâmetros) e [retouchui.rs](../crates/orvalith/src/retouchui.rs) (ferramentas de traço/clique; um traço = uma ação `LayersEdit`).

## Liquify (Estágio 10.2)
[liquify.rs](../crates/orvalith-core/src/liquify.rs): `Field` (deslocamento por pixel) com `push`, `smooth`, `reconstruct` e `render_region` (mapeamento inverso bilinear em alfa pré-multiplicado, sempre a partir do original). UI: [liquifyui.rs](../crates/orvalith/src/liquifyui.rs) mantém a sessão (original + campo) até Enter (uma ação `LayersEdit`) ou Esc.

## Estilos e preenchimentos (Estágio 11.1)
[style.rs](../crates/orvalith-core/src/style.rs): `Effect`/`EffectKind`, padrões e `apply` (conteúdo + efeitos numa região com margem). `compose.rs::blend_styled` monta o conteúdo (máscara e preenchimento no alfa), aplica os efeitos e mescla com o modo/opacidade da camada; a margem (`style::reach`) garante que renderizações parciais coincidam com a completa. `LayerSource::Fill(FillSpec)` ([source.rs](../crates/orvalith-core/src/source.rs)) dá as camadas de preenchimento. UI: [styleui.rs](../crates/orvalith/src/styleui.rs).

## Objetos inteligentes (Estágio 11.2)
[object.rs](../crates/orvalith-core/src/object.rs): `SmartObject` (original + centro/escala/giro + filtros + máscara) renderiza sempre a partir do original (filtros no espaço do original, depois transformação bilinear em alfa pré-multiplicado). `Layer.object` + `Layer::rerender` mantêm os pixels da camada; `vecops::translate_content` move texto/forma/objeto sem perda. UI: [objectui.rs](../crates/orvalith/src/objectui.rs); diálogos de arquivo em `app.rs`.

## Composições (Estágio 11.3)
[comps.rs](../crates/orvalith-core/src/comps.rs): `Comp` (estado visual por camada), `capture`/`apply`/`matches` e `CompsEdit` (comando de histórico). UI: [compsui.rs](../crates/orvalith/src/compsui.rs).

## Formatos (Estágio 12)
[formats.rs](../crates/orvalith-core/src/formats.rs): `Format`, tabela `FORMATS` (a mesma da documentação), `detect` (por conteúdo; TGA só pela extensão), `import`/`export` (+`ExportOpts`, avisos), SVG (resvg), OpenRaster (zip), `sprite_sheet`. `imageio.rs` guarda os codecs raster (`image`) e o tratamento de perfil ICC/animação/HDR.

## PDF (Estágio 12.2)
[pdf.rs](../crates/orvalith-core/src/pdf.rs): leitura/rasterização com `hayro` (senha local), escrita raster com `pdf-writer` (`PdfSetup`, `layout`). UI: [app/pdfs.rs](../crates/orvalith/src/app/pdfs.rs).

## PSD/PSB e .pcraft (Estágio 12.3–12.4)
[psd.rs](../crates/orvalith-core/src/psd.rs) (leitor/escritor próprios de PSD/PSB) e [pcraft.rs](../crates/orvalith-core/src/pcraft.rs) (adaptador somente-leitura do `.pcraft`, separado do `.orva`), ambos ligados por `formats.rs`.

## Animação (Estágio 13)
[anim.rs](../crates/orvalith-core/src/anim.rs): `Timeline` (quadros = composições + duração; trilhas de quadros-chave por camada/propriedade), `render_frame`/`render_onion`, `export_gif`, `png_sequence`, `TimelineEdit`. O canvas mostra `render_frame` quando `OpenDoc.anim_frame` está definido. UI: [app/timeline.rs](../crates/orvalith/src/app/timeline.rs).

## Vídeo e áudio (Estágio 14)
[media.rs](../crates/orvalith-core/src/media.rs) (FFmpeg/ffprobe isolados, `FrameCache`) e [video.rs](../crates/orvalith-core/src/video.rs) (`VideoProject`, `compose_frame`, `audio_plan`, `export`). Detalhes de instalação, licença e limites em [MEDIA.md](MEDIA.md). UI: [app/videoui.rs](../crates/orvalith/src/app/videoui.rs).

## Comandos e atalhos (Estágio 15.1)
[app/commands.rs](../crates/orvalith/src/app/commands.rs): `registry()` (comandos com id/rótulo/atalho padrão), `Keymap` (tabela ordenada que o teclado consulta), `rebind`/`effective` (diferenças guardadas em `prefs.keymap`), paleta e editor de atalhos.

## Ações e lote (Estágio 15.2)
[actions.rs](../crates/orvalith-core/src/actions.rs): `Step`/`Macro` (serializáveis), `apply_step`/`run`, `run_batch` (falhas isoladas por arquivo com `catch_unwind`), `report_text`. UI: [app/actionsui.rs](../crates/orvalith/src/app/actionsui.rs); a gravação recolhe `Applied` das sessões de ajuste/filtro e `GeoState::recorded`.

## Predefinições, espaços de trabalho e ajuda (Estágio 15.3)
[presets.rs](../crates/orvalith/src/presets.rs) (tipos, sanitização, pacote, loja global de gradientes), [app/workflow.rs](../crates/orvalith/src/app/workflow.rs) (janelas de predefinições, histórico/instantâneos, ajuda), núcleo: [resources.rs](../crates/orvalith-core/src/resources.rs) (GPL/GGR) e [examples.rs](../crates/orvalith-core/src/examples.rs).

## Estabilidade e distribuição (Estágio 16)
[robustness.rs](../crates/orvalith-core/src/robustness.rs) (testes de corrupção e medições), [crashlog.rs](../crates/orvalith/src/crashlog.rs) (gancho de pânico), [scripts/](../scripts/) (pacote e licenças). Veja [[DISTRIBUTION]] e [[PERFORMANCE]].

## Pendente
Tiles, compositor GPU (wgpu), jobs em segundo plano (salvar/abrir ainda bloqueiam a UI).
