"""Verify every staged file against committed evidence and launch without dev PATH."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
manifest = tomllib.loads((ROOT / "tools/manifest.toml").read_text())
baseline = json.loads((ROOT / "docs/evidence/phase0/bundle.json").read_text())
qualified = json.loads((ROOT / "docs/evidence/phase0/qualification.json").read_text())
if not qualified["gate"].startswith("passed"):
    raise RuntimeError("Qualification gate is open")
count = 0
for name, files in baseline["files"].items():
    base = ROOT / "tools/bin" / name
    actual_paths = {p.relative_to(base).as_posix() for p in base.rglob("*") if p.is_file()}
    if actual_paths != set(files):
        raise RuntimeError(f"{name}: staged file set differs from inventory")
    for relative, expected in files.items():
        with (base / relative).open("rb") as f:
            actual = hashlib.file_digest(f, "sha256").hexdigest()
        if actual != expected:
            raise RuntimeError(f"Modified/missing file: {name}/{relative}")
        count += 1
env = os.environ.copy()
env["PATH"] = str(Path(os.environ["SystemRoot"]) / "System32")
versions = {}
for name, tool in manifest["tools"].items():
    for executable in tool["executables"]:
        path = ROOT / "tools/bin" / tool["directory"] / executable
        if "binary_sha256" in tool:
            with path.open("rb") as f:
                actual = hashlib.file_digest(f, "sha256").hexdigest()
            if actual != tool["binary_sha256"]:
                raise RuntimeError(f"{name}: executable differs from qualified manifest pin")
        with path.open("rb") as f:
            actual = hashlib.file_digest(f, "sha256").hexdigest()
        if actual != qualified["binary_sha256"][path.name]:
            raise RuntimeError(f"{name}: run qualification against this exact rebuilt binary")
        result = subprocess.run([str(path), "--version"], env=env, cwd=path.parent,
                                capture_output=True, text=True, check=True, timeout=30)
        versions[f"{name}/{executable}"] = result.stdout.strip()
record = {"files_verified": count, "minimal_path_launch": "passed", "versions": versions,
          "scope": "Existing Windows host only; does not substitute for clean-machine installation tests"}
(ROOT / "docs/evidence/phase0/verification.json").write_text(json.dumps(record, indent=2) + "\n")
print(f"Verified {count} staged files; all four CLIs launch with only Windows System32 on PATH.")
