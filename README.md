# AIAS

AIAS 是一款 Windows 桌面贴图工具箱：PBR 通道合成与拆分、DDS 转换、Mipmap 生成、AI 抠图（内置离线 ONNX 推理），以及 War Thunder UserSkins 涂装管理。

桌面壳使用 Tauri 2.x，渲染层为原生 HTML/CSS/JavaScript。旧的桌面壳与 Python GUI 入口已不再包含在当前应用中。

## 功能

| 模式 | 说明 |
|---|---|
| PBR 多通道合成 | 生成游戏可用的 _c 与 _n 通道贴图 |
| PBR 多通道拆分 | 提取 BaseColor、Alpha、材质与法线通道 |
| Mipmap 生成 | 将分层图片序列组装为单个 DDS |
| 图片转 DDS | 批量转换图片并统一 DDS 压缩格式 |
| AI 抠图 | 动漫 / 人像 / 商品等任意图片智能抠图，输出透明背景 PNG |
| 涂装管理 | 管理 War Thunder UserSkins 资源 |
| 应用设置 | 更新、数据路径与版本信息 |

AI 抠图内置 6 个模型，全部按需下载、离线推理：AnimeSeg 动漫专精、ToonOut 动漫微调、BiRefNet 通用 1024、BiRefNet Lite 轻量、ISNet 动漫标准、RTMDet+精修 动漫精细。检测到 CUDA 环境时自动 GPU 加速，显存不足或依赖缺失时回退 CPU，保证出图。

## 环境要求

- Windows
- Node.js 20 或更新
- npm
- Visual Studio Code
- Tauri 2.x 所需的 Rust 工具链（`rustc` 与 `cargo`）
- Microsoft Visual Studio Build Tools（含「使用 C++ 的桌面开发」工作负载）
- Microsoft Edge WebView2 Runtime

## 安装

```powershell
npm install
```

## 开发运行（热重载）

```powershell
npm run tauri dev
```

会启动 Vite（`http://127.0.0.1:5173/`）并打开 Tauri 桌面窗口加载该渲染层。修改 `src/renderer` 下的文件会像普通 Web 应用一样热更新。

## 仅渲染层预览

```powershell
npm run dev:renderer
```

仅调整 UI 布局时使用。本地文件系统操作只在 Tauri 窗口内可用，纯浏览器预览中没有。

## 构建检查与安装包

```powershell
npm run check        # vite build，验证渲染层可构建
npm run build        # 产出 Windows 安装包
npm run build:fast   # 跳过 LTO 的快速打包（调试用）
```

安装包由 Tauri 生成在 `src-tauri/target/release/bundle/`，构建后自动复制到 `dist/`。安装包不入版本库，发版请上传到 Releases。

## VS Code

用 Visual Studio Code 打开本目录：

- 运行任务 `tauri: dev` 启动完整桌面应用。
- 运行任务 `renderer: dev` 进行纯 Web UI 预览。
- 在「运行和调试」面板使用 `AIAS Tauri Dev` 配置启动。

## 代码结构

```text
AIAS/
  src/
    renderer/
      index.html
      scripts/app.js
      styles/app.css
      styles/performance.css
  src-tauri/
    Cargo.toml
    tauri.conf.json
    build.rs
    capabilities/default.json
    src/main.rs            # Tauri 命令、设置、纹理管线、涂装管理
    src/anime.rs           # AI 抠图：模型注册表与公共入口
    src/anime/
      runtime.rs           # ORT DLL 获取、CUDA 运行时安装、会话缓存与显存回收
      infer.rs             # ISNet / RTMDet+精修 / BiRefNet 系三族推理管线
      postprocess.rs       # 引导滤波、实心化、幽灵抑制、去污染、孤岛清理
      tests.rs             # 单元测试与 AB/GT 基准
  tools/                   # 抠图质量评测：GT 对比、按轮次交付
  package.json
  vite.config.js
```

## 测试

```powershell
cd src-tauri
cargo test --release            # 单元测试（无需模型与 GPU）
```

`#[ignore]` 标记的是需要真实模型与 GPU 的手动基准（`ab_reference`、GT 对比、TTA/分块精修等实验），通过环境变量控制输入与输出目录，用法见 `tools/README.md`。改动抠图后处理管线时，建议先跑 `ab_reference`，再用 `tools/compare_gt.py` 与人工标准答案做量化对比。

## 说明

- 渲染层是原生 HTML/CSS/JavaScript：没有 TypeScript，也没有重量级前端框架。
- 桌面能力通过 Tauri 命令与官方 Tauri 插件提供。
- DDS 编解码与 Mipmap 生成全部在 Rust 侧完成，运行时不依赖外部纹理工具。
- AI 抠图的模型文件在首次使用时下载到应用数据目录，可在应用内卸载；模型来源与字节数校验写在 `anime.rs` 的模型注册表中。
