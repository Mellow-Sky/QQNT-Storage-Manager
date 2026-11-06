"""Package the no-dependency, optional manifest-v4 LiteLoader entrypoint."""
from pathlib import Path
import json
import zipfile

root = Path(__file__).resolve().parents[1]
source = root / "plugins" / "liteloader"
manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
assert manifest["manifest_version"] == 4 and manifest["platform"] == ["win32"]
target = root / "artifacts" / "qqnt-storage-manager-liteloader.zip"
target.parent.mkdir(exist_ok=True)
with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
    for name in ["manifest.json", "main.cjs", "preload.cjs", "renderer.js", "README.md"]:
        archive.write(source / name, name)
print(target)
