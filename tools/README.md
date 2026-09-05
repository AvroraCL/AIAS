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
#    上面两步也可以直接用一键脚本：tools/eval.sh <tag> [模型列表]
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

- **细节补全为正收益，已扩展到全身**（上半部 + ≤2048px 纵向分带，同双裁切门控）：
  相对不开启 mae 0.00358→0.00348，IoU 0.9930→0.9931，**内部漏检 -37%**
  （0.00139→0.00088；上半部方案只有 -21%），边界误差 0.1695→0.1685，
  外溢 +0.0003，耗时 4.0s→5.9s；加性不变量（只补不擦）由测试锁定。
- **ViTMatte 窄带精修在最难区域为负收益**（mae +25%、外溢 +0.025），保留为实验
  选项默认关闭；其门控（只许在基图边缘窄带内改动）由测试锁定。
- **闭式边界求解不推广到其它模型**：birefnet-general 加闭式后 mae/边界/外溢全部
  微增（如 0.04621→0.04649）；toonout 在真值图上必回退到 anime-specialist，无从
  评估。保持 anime-specialist 专属。
- **CPU 路径可用**：anime-specialist（int8 117MB）纯 CPU 端到端 15.5s（GPU 14s），
  指标与 GPU 几乎一致（mae 0.00368 vs 0.00358）——强桌面 CPU 上无显卡也有像样的
  默认模型；弱 CPU 未测。

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

## 高分辨率路线实测（2026-09-05，勿重复尝试）

「提高有效推理分辨率」的三条变体在真值图上均为负收益或接近无收益：

- **BiRefNet HR 2048 fp32 整图推理**：12GB RTX 4070 SUPER 上 CUDA EP 直接 OOM
  （BiasSoftmax 单笔 920MB 分配失败，重试仍崩）。目标用户显卡多低于此，作为
  正式模型不可行；官方与社区均无 HR fp16 导出。
- **边界分块精修**（全图 AnimeSeg 语义锚 + 2048px 原图块→1024 输入，1792 stride /
  256 feather，±48px 边界门）：50% 融合仅 boundary_mae 0.1727→0.1668（-3.4%），
  但 mae/IoU/外溢全面微降，耗时 3 倍（10.4s vs 3.5s）；100% 信任分块则大幅劣化
  （IoU 0.993→0.970，外溢 ×10）——分块缺全局上下文，重复抠背景。
- 结论：边界 2-3px 环的主导因素是模型语义不确定性与「贴线稿外沿切」的真值语义，
  不是输入分辨率。高分辨率路线全部关闭，精力转向细节补全全身化与 CPU 路径。
