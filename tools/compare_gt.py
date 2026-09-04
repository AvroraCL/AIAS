"""Compare AB cutout outputs against the human-made ground-truth cutout.

Usage: python compare_gt.py <tag> [model ...]
Reads F:/AIAS/ab/<tag>/原图_<model>.png and the GT alpha channel, reports:
  IoU      binary agreement (alpha>=128 vs GT>=128)
  ghost    % of GT-solid pixels we left semi-transparent (25<=a<230)  <- 用户主诉
  miss     % of GT-solid pixels we dropped entirely (a<25)
  residue  % of GT-empty pixels we kept (a>=25)
  edge_px  mean |our_alpha - GT_alpha| within 3px of the GT boundary
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image

GT_PATH = Path(r"F:/战争雷霆涂装/贴图素材/F15E 塞雷娅/测试/人工抠图版.png")
AB = Path("F:/AIAS/ab")


def boundary_band(gt_solid: np.ndarray, radius: int = 3) -> np.ndarray:
    """Pixels within `radius` of the GT boundary (dilate solid minus erode)."""
    a = gt_solid
    dil = a.copy()
    ero = a.copy()
    for _ in range(radius):
        dil[1:, :] |= dil[:-1, :]
        dil[:, 1:] |= dil[:, :-1]
        ero[:-1, :] &= ero[1:, :]
        ero[:, :-1] &= ero[:, 1:]
    return dil & ~ero


def main() -> None:
    tag = sys.argv[1] if len(sys.argv) > 1 else "p4ref"
    models = sys.argv[2:] or ["toonout", "simple", "advanced", "birefnet-general", "birefnet-lite"]

    gt = np.array(Image.open(GT_PATH))
    gt_a = gt[:, :, 3]
    gt_solid = gt_a >= 128
    gt_empty = ~gt_solid
    band = boundary_band(gt_solid)
    n_solid = gt_solid.sum()

    print(f"GT: solid {100 * gt_solid.mean():.1f}%  boundary-band px {band.sum()} ({100 * band.mean():.2f}%)")
    print(f"{'model':<18}{'IoU':>7}{'ghost%':>8}{'miss%':>7}{'resid%':>8}{'edge':>7}")
    for model in models:
        path = AB / tag / f"原图_{model}.png"
        if not path.exists():
            print(f"{model:<18}  (缺文件: {path.name})")
            continue
        ours = np.array(Image.open(path).convert("RGBA"))
        if ours.shape[:2] != gt_a.shape:
            ours = np.array(
                Image.open(path).convert("RGBA").resize((gt_a.shape[1], gt_a.shape[0]), Image.NEAREST)
            )
        a = ours[:, :, 3]
        solid = a >= 128
        inter = (solid & gt_solid).sum()
        union = (solid | gt_solid).sum()
        iou = inter / max(union, 1)
        mid = (a >= 25) & (a < 230)
        ghost = 100 * (mid & gt_solid).sum() / n_solid
        miss = 100 * ((a < 25) & gt_solid).sum() / n_solid
        resid = 100 * ((a >= 25) & gt_empty).sum() / gt_empty.sum()
        edge = np.abs(a.astype(int) - gt_a.astype(int))[band].mean() / 255
        print(f"{model:<18}{iou:>7.4f}{ghost:>8.2f}{miss:>7.2f}{resid:>8.2f}{edge:>7.3f}")


if __name__ == "__main__":
    main()
