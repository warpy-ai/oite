#!/usr/bin/env bash
# bundle-core-rolls.sh - Package the core rolls that ship with oitec.
#
# Usage: scripts/bundle-core-rolls.sh ROLLS_CHECKOUT OUTPUT.tar.gz
#
# ROLLS_CHECKOUT is warpy-ai/rolls at ROLLS_REF (scripts/release-pins.env).
# The archive holds rolls/<name>/roll.toml and rolls/<name>/src/ for each roll
# in CORE_ROLLS, plus rolls/MANIFEST ("@rolls/<name> <version>" per line);
# install.sh unpacks it into $OITE_HOME. Fails unless every core roll is on
# the registry at that version, and every dependency is itself a core roll,
# so what ships with oitec is exactly what the registry serves.
set -euo pipefail

ROLLS_DIR="${1:?usage: $0 ROLLS_CHECKOUT OUTPUT.tar.gz}"
OUTPUT="${2:?usage: $0 ROLLS_CHECKOUT OUTPUT.tar.gz}"
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=release-pins.env
. "$HERE/release-pins.env"
REGISTRY_URL="${UNROLL_REGISTRY:-https://registry.oite.org/api/v1}"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/rolls"

python3 - "$ROLLS_DIR" "$REGISTRY_URL" "$stage/rolls" $CORE_ROLLS <<'PY'
import json, shutil, sys, tomllib, urllib.parse, urllib.request
from pathlib import Path

rolls_dir, registry, out, *core = sys.argv[1:]
rolls_dir, out = Path(rolls_dir), Path(out)
names = {f"@rolls/{c}" for c in core}
errors, manifest = [], []
for c in core:
    toml = rolls_dir / c / "roll.toml"
    if not toml.exists():
        errors.append(f"{c}: no roll.toml in {rolls_dir}")
        continue
    data = tomllib.loads(toml.read_text())
    name, version = data["roll"]["name"], data["roll"]["version"]
    deps = dict(data.get("dependencies", {}))
    deps.update(deps.pop("optional", {}) if isinstance(deps.get("optional"), dict) else {})
    for dep in deps:
        if dep.startswith("@rolls/") and dep not in names:
            errors.append(f"{name} depends on {dep}, which is not a core roll")
    url = f"{registry}/crates/{urllib.parse.quote(name.lstrip('@'), safe='')}"
    req = urllib.request.Request(url, headers={"User-Agent": "oite-release/1 (+https://github.com/warpy-ai/oite)"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            published = json.load(r).get("versions", [])
    except Exception as e:
        errors.append(f"{name}: cannot read it from the registry ({e})")
        continue
    if version not in published:
        errors.append(f"{name} {version} is not on the registry (published: {', '.join(published) or 'none'})")
    (out / c).mkdir(parents=True)
    shutil.copy(toml, out / c / "roll.toml")
    shutil.copytree(rolls_dir / c / "src", out / c / "src")
    manifest.append(f"{name} {version}")
if errors:
    for e in errors:
        print(f"error: {e}", file=sys.stderr)
    sys.exit(1)
(out / "MANIFEST").write_text("\n".join(manifest) + "\n")
print("\n".join(manifest))
PY

tar -czf "$OUTPUT" -C "$stage" rolls
echo "Wrote $OUTPUT"
