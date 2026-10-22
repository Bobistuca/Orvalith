# Checklist de commit

1. `git status` e revisão do diff completo (sem depuração, caminhos locais, segredos, arquivos pessoais).
2. `git diff --check`; após `git add` seletivo, `git diff --cached --check`.
3. `cargo fmt --all -- --check`
4. `cargo clippy --workspace --all-targets --locked -- -D warnings`
5. `cargo test --workspace --locked` (confirmar que testes rodaram, não suite vazia)
6. `cargo build --release --locked` quando relevante.
7. Atualizar [[docs/CONTINUITY]], [[docs/FEATURE_MATRIX]], [[CHANGELOG]].
8. Commit descritivo; `git push origin main`; relatar o resultado real.

Mudanças só de documentação: validar links e diff, sem recompilar (registrar isso).
