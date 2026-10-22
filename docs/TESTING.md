# Testes

Comandos em [[CLAUDE]] e [[docs/COMMIT_CHECKLIST]].

- Automatizados: `cargo test --workspace --locked` (núcleo: composição alfa, histórico, validação de dimensões).
- Manual (UI): `cargo run -p orvalith` → "Novo documento" → criar 1920×1080 → ver xadrez/fundo branco; Ctrl+N abre o diálogo.
- Última execução registrada: ver [[docs/CONTINUITY]].

## Estágio 1 (2026-10-22)
fmt, clippy, 16 testes, build release OK; smoke test: abre, fecha, grava preferências. Procedimento manual: Ctrl+N ×2 → abas; roda do mouse = zoom no cursor; Espaço+arrastar ou botão do meio = pan; Ctrl+0/Ctrl+1; × fecha aba; Editar → Preferências (escala/tema) e reabrir o app.

## Estágio 0 (2026-10-22)
fmt, clippy -D warnings, 8 testes do núcleo e build release OK; executável release abre a janela "Orvalith" (verificado por processo/título; sem captura de tela nem teste de interação). Teste com rede bloqueada: pendente.
