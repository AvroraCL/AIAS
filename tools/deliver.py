"""Deliver AB outputs to the user's comparison folder with P-number suffixes.

Usage: python deliver.py <tag> <Pn> [model ...]
Copies F:/AIAS/ab/<tag>/{stem}_{model}.png -> AB测试结果/{stem}_{model}_<Pn>.png
       F:/AIAS/ab/<tag>/{stem}_{model}_preview.jpg -> AB测试结果/{stem}_{model}_preview_<Pn>.jpg
"""
import shutil
import sys
from pathlib import Path

AB = Path("F:/AIAS/ab")
DEST = Path(r"F:/战争雷霆涂装/贴图素材/F15E 塞雷娅/AB测试结果")


def main() -> None:
    tag, pn = sys.argv[1], sys.argv[2]
    models = sys.argv[3:] or ["toonout", "simple", "advanced", "birefnet-general", "birefnet-lite"]
    DEST.mkdir(exist_ok=True)
    stems = sorted({p.name.rsplit(f"_{m}", 1)[0] for m in models for p in AB.glob(f"{tag}/*_{m}.png")})
    for stem in stems:
        for model in models:
            src = AB / tag / f"{stem}_{model}.png"
            if src.exists():
                shutil.copy2(src, DEST / f"{stem}_{model}_{pn}.png")
            prev = AB / tag / f"{stem}_{model}_preview.jpg"
            if prev.exists():
                shutil.copy2(prev, DEST / f"{stem}_{model}_preview_{pn}.jpg")
            print(f"{stem}_{model}_{pn} ✓")


if __name__ == "__main__":
    main()
