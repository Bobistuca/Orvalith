"""Gera THIRD_PARTY_LICENSES.md a partir do Cargo.lock (`cargo metadata --locked`)."""
import collections
import json
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
out = subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--locked"],
    cwd=root, capture_output=True, text=True, check=True,
).stdout
meta = json.loads(out)
ws = set(meta["workspace_members"])
pk = [p for p in meta["packages"] if p["id"] not in ws]
lic = lambda p: p.get("license") or "DESCONHECIDA"
by = collections.Counter(lic(p) for p in pk)
lines = [
    "# Licenças de terceiros", "",
    "Gerado por `scripts/third_party.py` a partir do `Cargo.lock`. Cada dependência mantém a própria licença; os textos completos acompanham o código-fonte de cada pacote (crates.io).", "",
    "O FFmpeg **não** é distribuído com o Orvalith: é usado como programa externo já instalado no computador do usuário (ver `docs/MEDIA.md`).", "",
    f"Total: {len(pk)} pacotes.", "",
    "## Resumo por licença", "", "| Licença (SPDX) | Pacotes |", "|---|---|",
]
lines += [f"| {k} | {v} |" for k, v in by.most_common()]
lines += ["", "## Pacotes", "", "| Pacote | Versão | Licença |", "|---|---|---|"]
lines += [f"| {p['name']} | {p['version']} | {lic(p)} |" for p in sorted(pk, key=lambda p: (p["name"], p["version"]))]
(root / "THIRD_PARTY_LICENSES.md").write_text("\n".join(lines) + "\n", encoding="utf8", newline="\n")
unknown = [p["name"] for p in pk if not p.get("license")]
print(len(pk), "pacotes;", "sem licença declarada:", unknown or "nenhum")
sys.exit(0)
