---
type: politica
status: ativo
tags: [orvalith, privacidade]
---
# Armazenamento local

- Sem login, conta, backend, sincronização, upload, telemetria ou envio de logs.
- Abrir, editar, renderizar PDF, salvar e exportar funcionam sem internet.
- Arquivos só são gravados em caminhos escolhidos pelo usuário (Salvar/Exportar explícitos).
- Autosave/recuperação: opcional, **desligado por padrão** (a implementar no Estágio 2).
- Preferências: arquivo de configuração local (a implementar); lista de recentes opcional e apagável.
- Temporários/caches: diretório local do aplicativo com limite de tamanho e limpeza (a implementar).
- Renderizadores (PDF/SVG) não buscam recursos remotos.

**Diretórios reais hoje:** apenas `%APPDATA%\Orvalith\config.json` (Linux/macOS: `~/.config/orvalith/config.json`) com tema, escala da UI e tamanho da janela, gravado de forma atômica ao fechar ou alterar preferências. Nenhum documento é gravado.

Git/GitHub é usado apenas no desenvolvimento; não faz parte do aplicativo distribuído. Teste com rede bloqueada: pendente.
