#!/usr/bin/env bash
# 一键抠图评测：跑 ab_reference 基准 → 与人工真值对比输出指标表。
# 用法:
#   tools/eval.sh <tag> [模型列表]        # 模型列表逗号分隔，默认 anime-specialist
# 可选环境变量:
#   AIAS_AB_INPUT   输入图片或目录（默认 F:/AIAS/ab/inref，只放原图不放 GT）
#   AIAS_AB_DEBUG   任意非空值时额外导出 _matte_raw.png / _matte.png
#   AIAS_AB_GT      真值路径（默认 F15E 塞雷娅 测试目录的人工抠图版.png）
# 需要 cargo 在 PATH 中（或先 source 你的 Rust 环境）。
set -euo pipefail

TAG="${1:?用法: tools/eval.sh <tag> [模型列表]}"
MODELS="${2:-anime-specialist}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
INPUT="${AIAS_AB_INPUT:-$REPO/ab/inref}"
GT="${AIAS_AB_GT:-F:/战争雷霆涂装/贴图素材/F15E 塞雷娅/测试/人工抠图版.png}"

cd "$REPO/src-tauri"
AIAS_AB_INPUT="$INPUT" AIAS_AB_GT="$GT" AIAS_AB_MODELS="$MODELS" AIAS_AB_TAG="$TAG" \
  cargo test --release anime::tests::toonout_tests::ab_reference -- --ignored --exact --nocapture

cd "$REPO"
python tools/compare_gt.py "$TAG" $MODELS
