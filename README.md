<div align="center">

<img src="docs/AIAS-ico.png" alt="AIAS — 面向 War Thunder 涂装作者的 Windows 贴图工作台" width="140">

[![release](https://img.shields.io/github/v/release/AvroraCL/AIAS?style=flat-square&logo=github)](https://github.com/AvroraCL/AIAS/releases)
[![Windows](https://img.shields.io/badge/Windows-10%20%2F%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](https://github.com/AvroraCL/AIAS)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white)](https://github.com/AvroraCL/AIAS)
[![Rust](https://img.shields.io/badge/Rust-2021%20edition-DEA584?style=flat-square&logo=rust&logoColor=white)](https://github.com/AvroraCL/AIAS)
[![Node.js](https://img.shields.io/badge/Node.js-20%2B-339933?style=flat-square&logo=nodedotjs&logoColor=white)](https://github.com/AvroraCL/AIAS)
[![license](https://img.shields.io/github/license/AvroraCL/AIAS?style=flat-square)](LICENSE)

面向 War Thunder 涂装作者的 Windows 贴图工作台

</div>

AIAS 把 PBR 通道合成与拆分、Mipmap 生成、DDS 批量转换、AI 抠图和涂装管理装进一个开箱即用的 Windows 应用，无需命令行，也不依赖 ComfyUI、Python 环境或在线服务；图像编解码与模型推理全部在本机离线完成。

> AIAS 是独立社区工具，与 Gaijin Entertainment 无从属或背书关系。

## 面向用户

### AIAS 能做什么

- PBR 合成 / 拆分：把颜色、透明、材质、法线等单通道图一键合成游戏可用的通道贴图，或从已有贴图反向拆出各通道
- Mipmap 生成：把分层图片序列组装为带完整 Mipmap 链的单个 DDS，支持 8K→6K/4K 等游戏友好缩放
- 图片转 DDS：批量转换图片并统一 BCn 压缩格式
- AI 抠图：动漫 / 人像 / 商品等任意图片智能抠图，输出透明背景 PNG；内置 6 个离线模型（AnimeSeg 动漫专精、ToonOut 动漫微调、BiRefNet 通用 1024、BiRefNet Lite 轻量、ISNet 动漫标准、RTMDet+精修 动漫精细），全部按需下载、可在应用内卸载；检测到 CUDA 环境时自动 GPU 加速，显存不足或依赖缺失时自动回退 CPU 保证出图；处理大图时有内置资源监控与对比视图
- 涂装管理：自动检测 War Thunder UserSkins 目录，导入、启停、删除涂装
- 启动时自动检查新版本，一键下载安装并升级（GitHub Releases 主源，不可达时自动回退 GitCode 镜像）

### 下载 v5.4.4

| 文件 | 说明 |
|---|---|
| [AIAS_5.4.4_x64-setup.exe](https://github.com/AvroraCL/AIAS/releases/download/v5.4.4/AIAS_5.4.4_x64-setup.exe) | 中文安装向导，按当前用户安装，无需管理员权限 |

历史版本与更新日志见 [Releases](https://github.com/AvroraCL/AIAS/releases)。

### 当前状态

AIAS 的贴图管线（PBR 合成 / 拆分、Mipmap、DDS 转换）与涂装管理已稳定可用，适合涂装作者日常使用。AI 抠图处于活跃迭代期：5.4 版接入 AnimeSeg 动漫专精与官方 BiRefNet 通用模型，并持续精修边缘质量——受推理分辨率限制，极细发丝与线稿边界仍有约 2-3 像素的精度上限，导出前建议先在对比视图中确认效果。

### 系统要求

- Windows 10 或 Windows 11（x64）
- WebView2 Runtime（Windows 10/11 通常已经预装）
- AI 抠图 GPU 加速需要 NVIDIA 显卡（CUDA 12 / cuDNN 9，可在应用内一键安装）；无独显或显存不足时自动回退 CPU 推理，较慢但可用

正式版本已内嵌图像编解码与 ONNX Runtime；抠图模型体积较大（数百 MB 到 1 GB），首次使用时按需下载到应用数据目录。

### 安装与使用

从上方下载区或 [Releases](https://github.com/AvroraCL/AIAS/releases) 页面获取安装包。

启动后，典型的涂装制作流程是：

1. 添加需要处理的图片（PNG / JPG / TGA / DDS）。
2. 选择模式：PBR 合成、PBR 拆分、Mipmap、图片转 DDS、AI 抠图或涂装管理。
3. 确认参数并执行，结果写入指定目录，预览即时可见。
4. 用涂装管理把成品导入游戏的 UserSkins 目录。

AI 抠图首次选择某个模型时会自动下载；下载不可达时自动切换镜像重试。

### 已知限制

- 当前版本尚未进行代码签名，首次运行 Windows SmartScreen 可能显示安全提醒。
- AI 抠图在 512/1024 分辨率上推理，极细发丝与线稿边界存在约 2-3 像素的精度上限。
- GPU 加速仅支持 NVIDIA 显卡；AMD / Intel 显卡使用 CPU 推理。
- 模型文件体积较大，首次使用需要等待下载。

### 架构约束

渲染层只负责交互、参数与预览；图像编解码、DDS 压缩、模型下载与推理、后处理精修全部由 Rust 侧模块拥有，通过 Tauri IPC 调用。前端不复制任何图像算法，同一段业务逻辑与数值行为保持单一实现。

---

## 面向开发者

### 设计原则

> **离线优先，一次装好。**

AIAS 的全部功能——贴图处理与模型推理——都在本机完成：不需要 ComfyUI、Python 或在线 API。模型按需下载后即可完全离线使用；运行时依赖（ONNX Runtime、CUDA 运行时）由应用自动获取与回收，不给用户留命令行。

### 技术栈

- Tauri 2（窗口与 IPC）
- Rust（图像处理与推理宿主）
- `ort`（ONNX Runtime，load-dynamic，可选 CUDA ExecutionProvider）
- `image` / `image-dds`（编解码与 BCn 压缩）
- 原生 HTML/CSS/JS + Vite（渲染层，无前端框架）

仓库早期把全部抠图逻辑放在单文件 `anime.rs` 中。2026-09 拆分为 `anime/` 模块目录（注册表、运行时、推理、后处理、测试），如需参考旧实现可从历史提交获取。

### 开发环境

- Windows 10/11 x64
- Node.js 20+ 与 npm
- Rust 工具链（MSVC）与 Visual Studio Build Tools（「使用 C++ 的桌面开发」工作负载）
- WebView2 Runtime
- （可选）NVIDIA GPU + CUDA 12 / cuDNN 9，用于 GPU 推理调试

统一开发入口：

```bash
npm run dev           # 桌面开发模式：Rust 增量编译 + 渲染层热重载
npm run dev:renderer  # 仅渲染层浏览器预览（内置 mock 数据，无后端）
```

常用命令：

```bash
npm run check        # vite build，验证渲染层可构建
npm run build        # 正式打包（NSIS 安装包）
npm run build:fast   # 跳过 LTO 的快速打包（调试用）
cd src-tauri && cargo test --release   # Rust 回归测试（无需模型与 GPU）
```

`dev:renderer` 用于纯 UI 改动：浏览器中界面可交互，但本地文件系统操作只在 Tauri 窗口内可用。`cargo test` 包含抠图管线的单元测试与 A/B / GT 基准；`#[ignore]` 标记的手动基准通过环境变量控制输入输出，用法见 [tools/README.md](tools/README.md)。

### 运行流程

`npm run dev` 的预期行为：

1. 启动 Vite（`http://127.0.0.1:5183/`）并增量编译 Rust 侧。
2. 打开 Tauri 桌面窗口，加载渲染层。
3. 渲染层的图片操作经 Tauri IPC 进入 Rust 命令，结果写回用户指定目录并回传预览。
4. 修改 `src/renderer` 下的文件即时热更新；修改 Rust 代码保存后自动重编译并重启窗口。

### 目录结构

```text
src/renderer/                      # 渲染层（原生 HTML/CSS/JS + Vite）
├── index.html                     # 六个模式的界面骨架
├── scripts/app.js                 # 模式切换、IPC 调用与预览逻辑
└── styles/                        # 样式与性能模式样式
src-tauri/src/
├── main.rs                        # Tauri 命令、设置、纹理管线、涂装管理
├── anime.rs                       # AI 抠图：模型注册表、回退调度与公共入口
└── anime/
    ├── runtime.rs                 # ORT DLL 获取、CUDA 运行时安装、会话缓存与显存回收
    ├── infer.rs                   # ISNet / RTMDet+精修 / BiRefNet 系三族推理管线
    ├── postprocess.rs             # 引导滤波、实心化、幽灵抑制、去污染、孤岛清理
    └── tests.rs                   # 单元测试与 AB/GT 基准
tools/                             # 开发期质量工具：GT 对比评测、按轮次交付
```

依赖方向固定为 `main.rs → anime.rs → anime/*`：渲染层只经 IPC 与 Rust 通信；`tools/` 不参与应用构建，仅供开发期质量验证。

### 构建与打包

```bash
npm run build
```

安装包由 Tauri 生成在 `src-tauri/target/release/bundle/nsis/`，构建后自动复制到 `dist/`：

- `AIAS_{版本}_x64-setup.exe`：NSIS 安装程序

安装包不入版本库，发版上传到 Releases。版本信息在 `package.json`、`src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml` 三处同步维护。应用内置自动更新：启动时检查 GitHub Releases 上的 `latest.json`，不可达时回退 GitCode 镜像。

### 验证场景

| 场景 | 操作 | 预期结果 |
|---|---|---|
| PBR 合成 | 添加多张单通道图并执行 | 生成游戏可用的通道贴图，预览可核对各通道 |
| 图片转 DDS | 批量添加图片并选择 BCn 格式 | 全部转换成功并带完整 Mipmap 链 |
| AI 抠图首次使用 | 选择一个未下载的模型 | 自动下载模型并完成推理，状态变为已安装 |
| GPU 回退 | 无 NVIDIA 显卡或显存不足时抠图 | 自动回退 CPU 推理并正常出图 |
| 涂装管理 | 打开涂装管理检测游戏目录 | 列出已有涂装，可导入、启停与删除 |
| 仅前端改动 | `npm run dev:renderer` | 浏览器中界面可交互（mock 数据），无需后端 |

### 参与开发

欢迎通过 Issue 提交问题、建议与复现步骤。改动必须保持「离线优先，一次装好」的定位：不引入在线服务或运行时依赖，图像处理与推理留在 Rust 侧，并保证 `cargo test` 与 `npm run check` 通过。
