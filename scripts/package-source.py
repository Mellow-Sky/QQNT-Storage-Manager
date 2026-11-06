"""Package a reproducible source snapshot without dependencies or generated data."""
from pathlib import Path
import json
import os
import zipfile

root = Path(__file__).resolve().parents[1]
version = json.loads((root / "package.json").read_text())["version"]
output = root / "artifacts" / f"qqnt-storage-manager-{version}-source.zip"
output.parent.mkdir(exist_ok=True)
folders = {".github", "apps", "crates", "docs", "licenses", "plugins", "scripts", "tests"}
root_files = {".gitattributes", ".gitignore", "Cargo.toml", "Cargo.lock", "LICENSE", "README.md", "THIRD_PARTY_NOTICES.md", "package.json", "package-lock.json", "playwright.config.ts", "prompt.md", "rustfmt.toml"}
excluded = {"node_modules", "dist", "target", "gen", "__pycache__", "test-results", "playwright-report"}
with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
    for directory, names, files in os.walk(root):
        parent = Path(directory)
        names[:] = sorted(name for name in names if name not in excluded and (parent != root or name in folders))
        for name in sorted(files):
            path = parent / name
            relative = path.relative_to(root)
            if parent == root and name not in root_files:
                continue
            if path.suffix in {".zip", ".log", ".db", ".deb", ".tsbuildinfo", ".exe", ".dll", ".lib", ".pdb", ".so", ".o"}:
                continue
            archive.write(path, str(Path(f"qqnt-storage-manager-{version}") / relative))
print(output)
