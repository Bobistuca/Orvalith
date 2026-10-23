---
type: politica
status: ativo
tags: [orvalith, privacidade]
---
# Armazenamento local

- Sem login, conta, backend, sincronização, upload, telemetria ou envio de logs.
- Abrir, editar, renderizar PDF, salvar e exportar funcionam sem internet.
- Arquivos só são gravados em caminhos escolhidos pelo usuário (Salvar/Exportar explícitos).
- Autosave/recuperação: opcional, **desligado por padrão**; cópias `recovery-<pid>-<id>.orva` na pasta de recuperação (padrão `%APPDATA%\Orvalith\recovery`, configurável), removidas ao salvar/fechar o documento ou sair; botão "Apagar cópias de recuperação" nas preferências. Ao iniciar, cópias de sessões anteriores geram um diálogo (Recuperar/Descartar).
- Preferências: `config.json` local; lista de recentes opcional e apagável (guarda caminhos de arquivos).
- Temporários/caches: diretório local do aplicativo com limite de tamanho e limpeza (a implementar).
- Renderizadores (PDF/SVG) não buscam recursos remotos.

**Diretórios reais hoje:** apenas `%APPDATA%\Orvalith\config.json` (Linux/macOS: `~/.config/orvalith/config.json`) com tema, escala da UI e tamanho da janela, gravado de forma atômica ao fechar ou alterar preferências. Documentos só são gravados onde o usuário escolhe em Salvar/Exportar (arquivo temporário vizinho `*.orvatmp` durante a gravação, removido em seguida) e, se ativado, na pasta de recuperação.

Git/GitHub é usado apenas no desenvolvimento; não faz parte do aplicativo distribuído. Teste com rede bloqueada: pendente.
