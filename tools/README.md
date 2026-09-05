# 抠图质量评测工具（AB 基准 + 人工 GT 对比）

这套流程用于在改动 `anime.rs` 后处理管线时获得**客观数字**，代替目检猜测。

## 组成

- `compare_gt.py` — 把 AB 输出与人工抠图标准答案逐像素对比，报告 IoU / ghost% / miss% / resid% / edge。
- `deliver.py` — 把 AB 输出按 P 序号（P1、P2…）拷贝到用户的 `AB测试结果` 文件夹，便于人工逐轮对比。
- AB 输出本身由 `src-tauri/src/anime.rs` 里 `#[ignore]` 的 `ab_reference` 测试产生，不入库（`ab/` 已 ignore）。

## GT 获取

人工标准答案由用户提供：一张原图 + 一张人工抠图 PNG（`F:\战争雷霆涂装\贴图素材\F15E 塞雷娅\测试\` 下的
`原图.png` 与 `人工抠图版.png`，4268×4785，RGBA）。GT 蒙版几乎全二值（中间透明度 0.11%），
即「主体全实心、贴线稿外沿切、边缘极薄」——这就是质量目标。

注意：请用户再提供新的参考对时，把两张图各复制一份到 `F:\AIAS\ab\inref\`（只放原图，不放 GT，
否则 `ab_reference` 会把 GT 也当输入跑一遍）。

## 工作流

```bash
# 1. 跑基准（默认全部 6 个模型；可用 AIAS_AB_MODELS 限定）
cd src-tauri
AIAS_AB_TAG=exp1 AIAS_AB_INPUT="F:/AIAS/ab/inref" \
  AIAS_AB_MODELS="birefnet-general" \
  cargo test ab_reference --release -- --ignored --nocapture

# 2. 与 GT 对比（tag 对应第 1 步的输出目录 F:/AIAS/ab/exp1）
python tools/compare_gt.py exp1

# 3. 满意后按轮次交付给用户人工复核
python tools/deliver.py exp1 P5
```

调试辅助：`AIAS_AB_DEBUG=1` 额外导出引导滤波前的原始蒙版（`*_matte_raw.png`）与
最终蒙版（`*_matte.png`），用于定位某一步后处理引入的偏差。

## 实验开关基准（anime-specialist 专属，默认关闭）

两个用户可见的实验选项各有对应的 `#[ignore]` 基准：

```bash
# 细节补全（两次上半部局部推理，只补不擦）：全图指标见输出 report.txt
cargo test --release anime::recovery_tests::ab_production_detail_recovery_path -- --ignored --exact --nocapture

# ViTMatte 发丝探针：需要 tmp/vitmatte-small/model.onnx（约 99MB）与
# anime-specialist 的 _matte_raw.png（AIAS_AB_DEBUG=1 产生）作为基 alpha。
# AIAS_AB_CROP=x,y,w,h 指定评估窗口——自动选窗可能落在毫无误差的区域，测不出差异。
AIAS_AB_BASE_ALPHA=... AIAS_AB_PRODUCT_RESULT=... \
  cargo test --release anime::tests::toonout_tests::ab_vitmatte_local_hair_probe -- --ignored --exact --nocapture
```

结论（2026-09，真值图实测）：

- **细节补全为正收益**：mae 0.00358→0.00339，IoU 0.9930→0.9934，内部漏检 -21%，
  边界误差 0.1695→0.1648，外溢不变；加性不变量（只补不擦）由测试锁定。
- **ViTMatte 窄带精修在最难区域为负收益**（mae +25%、外溢 +0.025），保留为实验
  选项默认关闭；其门控（只许在基图边缘窄带内改动）由测试锁定。

## 指标定义

设 GT 实心 = `gt_alpha >= 128`，我们的实心 = `alpha >= 128`：

| 指标 | 含义 |
|---|---|
| IoU | 实心区域交并比 |
| ghost% | GT 实心里我们给了半透明（25≤a<230）的比例——**「幽灵发丝」主诉的直接度量** |
| miss% | GT 实心里我们整块丢掉（a<25）的比例 |
| resid% | GT 背景里我们保留（a≥25）的比例 |
| edge | GT 边界 3px 带内 alpha 差的均值（0-1） |

## 已有结论（2026-09，勿重复尝试）

以下方案在 GT 上实测为负收益或无判别力，边界 2-3px 环是 512² 推理的分辨率上限：

- 引导滤波加大半径（8→24）或改二值种子输入：边界几乎不动
- sigmoid 阈值内移（0.5→0.85）：IoU 单调下降，resid 换 miss 一比一
- 线稿吸附（暗线阻隔判定）：主体内部暗色细节与线稿无法区分，误伤大于修复
- 测地拓扑判据（线稿包围）：白对白无线路段洪水渗漏，无判别力
- 内部空洞颜色填充（包围孔 + 颜色相似）：均值颜色无判别力，唯一正收益场景误伤 6k px
