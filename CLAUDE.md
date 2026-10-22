# Orvalith — instruções permanentes

Editor de imagens desktop em Rust (egui/eframe), offline, sem login. Especificação original: `PROMPT_ORVALITH.md` (colado pelo usuário; não versionado se contiver dados pessoais).

## Ordem de leitura ao retomar
1. [[docs/CONTINUITY]] (estado atual e próximo passo) → 2. [[ROADMAP]] → 3. [[docs/FEATURE_MATRIX]] → 4. módulos relevantes. Código e testes prevalecem sobre notas desatualizadas.

## Comandos
Rust fica em `%USERPROFILE%\.cargo\bin` (pode não estar no PATH da sessão).
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked
cargo run -p orvalith
```

## Regras
- Núcleo em `crates/orvalith-core` (sem UI); UI em `crates/orvalith` só chama o núcleo.
- Sem rede, login, telemetria ou IA no app ([[docs/LOCAL_STORAGE]]).
- Recurso só é "validated" com teste/evidência; ver [[docs/COMMIT_CHECKLIST]].
- Commit + push de cada mudança lógica validada em `main` (remote `origin`). Sem force push, sem `--no-verify`, sem identidade global alterada.
- Nunca versionar arquivos pessoais; a pasta `Orvalith/` (cofre Obsidian pessoal) está no `.gitignore`.
- Atualizar CONTINUITY, FEATURE_MATRIX e CHANGELOG a cada checkpoint.
