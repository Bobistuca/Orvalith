# Vídeo e áudio (Estágio 14) — FFmpeg

O Orvalith **não inclui nem vincula** o FFmpeg. Ele executa os programas `ffmpeg` e `ffprobe` já instalados no computador (processos externos, com argumentos estruturados, sem shell). Todo o código de mídia fica isolado em [media.rs](../crates/orvalith-core/src/media.rs) (detecção, `ffprobe`, extração de quadros, cache) e [video.rs](../crates/orvalith-core/src/video.rs) (projeto, composição, áudio, exportação).

## Instalação (Windows)
```
winget install Gyan.FFmpeg
```
Ou baixe uma compilação de https://www.ffmpeg.org/download.html e: ponha `ffmpeg.exe`/`ffprobe.exe` no `PATH`, ou defina a variável `ORVALITH_FFMPEG_DIR` com a pasta que os contém. O aplicativo detecta a versão e os codificadores ao abrir a janela **Animação → Vídeo…** (H.264, AAC, VP9, Opus) e mostra o resumo.

## Licenças
* O FFmpeg é licenciado sob **LGPL 2.1+** ou **GPL 2+/3+** conforme as opções de compilação (a compilação `full` do gyan.dev usa `--enable-gpl`; o aplicativo informa "compilação GPL/LGPL"). Como o Orvalith apenas **executa** o programa externo (sem vincular bibliotecas), a licença do FFmpeg vale para ele, não para o Orvalith.
* Se um dia o FFmpeg for **distribuído junto** com o Orvalith, será preciso cumprir a licença da compilação escolhida (texto da licença, aviso, código-fonte correspondente para GPL). O instalador/portável do Estágio 16 deve decidir isso. Codificadores como H.264 (x264) e AAC podem envolver patentes em algumas jurisdições; a alternativa **WebM (VP9 + Opus)** tem licenciamento aberto.
* Nenhum dado é enviado à rede: os comandos usam apenas arquivos locais.

## O que foi implementado
* **Importar** vídeo e áudio com metadados reais (`ffprobe`: duração, codecs, dimensões, taxa de quadros, áudio), **miniaturas/prévia por quadro** com cache LRU.
* **Projeto de vídeo** anexado ao documento (`.orva` v12): trilhas de vídeo e de áudio, clipes de mídia e **imagens/títulos** (o documento atual composto vira um clipe com transparência, por cima do vídeo).
* **Tempos distintos**: quadro do projeto (`ProjectFrame`), carimbo dentro da mídia (`MediaSeconds`); o quadro de origem nunca é guardado.
* **Cortar** (no cabeçote), **aparar o início/fim arrastando as bordas**, **mover arrastando**, excluir; limites pela duração da mídia.
* **Transformação** (posição, escala), **opacidade** e **ajuste de cor** (qualquer ajuste do Estágio 9) por clipe.
* **Transições**: fade de entrada/saída e **dissolver** (sobreposição com fade) — um único mecanismo de rampas.
* **Áudio**: volume por clipe e por trilha, silenciar trilha, fades, atraso; mixagem pelo FFmpeg (`amix`) sincronizada com o vídeo.
* **Exportação** com progresso e cancelamento (remove o arquivo parcial): MP4 (H.264 + AAC) e WebM (VP9 + Opus), resolução, taxa de quadros e qualidade (CRF). Ao terminar, o `ffprobe` verifica o arquivo gerado.
* **Mídia relativa e ausente**: o projeto guarda o caminho absoluto e o relativo à pasta do `.orva`; ao abrir, um arquivo movido junto com o projeto é encontrado; mídia ausente é listada com **Localizar…** (relink).

## Limitações declaradas
* Sem reprodução em tempo real com som dentro do aplicativo (a prévia é por quadro; o áudio só é mixado na exportação).
* Velocidade de clipe fixa em 1×; sem curvas, rastreamento, legendas, efeitos cinematográficos, 3D ou colaboração (fora de escopo do estágio).
* Cada decodificador de clipe roda como um processo `ffmpeg`; muitos clipes simultâneos são lentos.
* Áudio sempre mixado para estéreo 48 kHz.
* Testado com clipes sintéticos gerados pelo próprio FFmpeg (cores sólidas e tons senoidais); reprodução em players externos independentes fica como verificação manual (o `ffprobe` e a re-extração de quadros foram usados nos testes automatizados).
