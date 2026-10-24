---
type: distribuicao
status: ativo
tags: [orvalith, distribuicao]
---
# Distribuição (Estágio 16)

## Gerar o pacote portátil (Windows x64)
```
powershell -ExecutionPolicy Bypass -File scripts\package.ps1
```
Faz `cargo build --release --locked`, monta `dist\Orvalith-<versão>-windows-x64\` (Orvalith.exe, LICENSE, README, CHANGELOG, THIRD_PARTY_LICENSES), compacta em `.zip` e grava o SHA-256 em `dist\SHA256SUMS.txt`. A pasta `dist/` não é versionada.

Para o usuário final: descompactar em qualquer pasta e executar `Orvalith.exe`. Não há instalador, serviço, tarefa agendada nem alteração de registro. Preferências, autosave e relatório de falhas ficam em `%APPDATA%\Orvalith`.

## Versão e licenças
- Versão única em `Cargo.toml` (`[workspace.package]`), atualmente 0.1.0 (pré-lançamento).
- Licença do projeto: MIT (`LICENSE`).
- Dependências: `THIRD_PARTY_LICENSES.md`, gerado por `python scripts/third_party.py`. Todas as 545 têm licença declarada e oferecem opção permissiva (MIT/Apache/BSD/Zlib/Unicode etc.); nenhuma exige copyleft.
- FFmpeg não é incluído: é um programa externo opcional que o usuário instala (veja [[MEDIA]]).

## Integração contínua
- `ci.yml`: fmt, clippy, testes e build release a cada push.
- `release.yml`: ao enviar uma tag `v*`, roda os testes e `package.ps1` e guarda o zip e o SHA-256 como artefatos. Não publica nada sozinho. **Não foi executado** (precisa de uma tag no GitHub).

## Não feito
- Instalador (MSI/MSIX), assinatura de código (o Windows SmartScreen pode avisar), ícone e metadados de versão no `.exe`, atualização automática (de propósito: o aplicativo não usa rede), builds para macOS/Linux.
- O executável de release usa o subsistema "windows" (sem janela de console).

## Falhas
Um pânico grava `crash.log` em `%APPDATA%\Orvalith` (texto, até 256 KiB, nada é enviado). O usuário pode apagar o arquivo ou anexá-lo a um relato de problema, se quiser.
