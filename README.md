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

AIAS 把 PBR 通道合成与拆分、Mipmap 生成、DDS 批量转换、风格实验室（25 套图片风格化）、AI 抠图与超分、模型烘焙和涂装管理装进一个开箱即用的 Windows 应用，无需命令行，也不依赖 ComfyUI、Python 环境或在线服务；图像编解码与模型推理全部在本机离线完成。

> AIAS 是独立社区工具，与 Gaijin Entertainment 无从属或背书关系。

## 面向用户

### AIAS 能做什么

- PBR 合成 / 拆分：把颜色、透明、材质、法线等单通道图一键合成游戏可用的通道贴图，或从已有贴图反向拆出各通道
- BLK 生成：扫描 DDS 目录，在紧凑行列表中直接编辑游戏原贴图映射（↑↓/Enter 行间移动，原名支持"文件名 / 去 _c/_n / 清空"批量填充），生成涂装配置；支持 C 文件批量指令、一图多映射与覆盖前差异确认
- Mipmap 生成：把分层图片序列组装为带完整 Mipmap 链的单个 DDS，支持 8K→6K/4K 等游戏友好缩放
- 图片转 DDS：批量转换图片并统一 BCn 压缩格式
- 风格实验室：25 套图片风格化一键出图——图像转 ASCII（字符/方块/波点/线稿）与 21 套风格化（故障艺术、像素排序、坏块流动、信号撕裂、数据腐蚀、CRT 显像管、信号重影、迷彩生成、油画厚涂、半调印刷、素描炭笔、热感假彩、十字绣、双色调、水彩晕染、低多边形、像素画、版画木刻、胶片颗粒、水波纹、玻璃折射、马赛克拼贴、热浪扭曲、蜡笔粉彩、幻彩全息），全部本地实时预览；数值可直接键入、标签拖动微调、双击单参复位、滚轮步进、随机探索，每套附出厂预设并支持自定义预设
- AI 抠图：动漫 / 人像 / 商品等任意图片智能抠图，输出透明背景 PNG；内置 6 个离线模型（AnimeSeg 动漫专精、ToonOut 动漫微调、BiRefNet 通用 1024、BiRefNet Lite 轻量、ISNet 动漫标准、RTMDet+精修 动漫精细），全部按需下载、可在应用内卸载；NVIDIA+CUDA Toolkit 环境自动满血 CUDA 加速，其他显卡可下载 DirectML 运行库（约 12 MB，全显卡通用）获得 GPU 加速，全部失败时自动回退 CPU 保证出图；处理大图时有内置资源监控与对比视图
- AI 超分：动漫 / 通用双模型本地 4x 超分，放大细节的同时保持画面干净
- 模型烘焙：为同一 OBJ/GLB/glTF 静态模型生成智能材质可识别的 AO、切线/世界空间法线、曲率、位置、厚度、ID 与 UV Mesh Map；支持智能 UV、DXR 光追、Intel OIDN 降噪、按材质输出
- 涂装管理：自动检测 War Thunder UserSkins 目录，导入、启停、删除涂装
- 启动时自动检查新版本，一键下载安装并升级（GitHub Releases 主源，不可达时自动回退 GitCode 镜像）

### 下载 v5.7.0

| 文件 | 说明 |
|---|---|
| [AIAS_5.7.0_x64-setup.exe](https://github.com/AvroraCL/AIAS/releases/download/v5.7.0/AIAS_5.7.0_x64-setup.exe) | Windows 安装包，按当前用户安装无需管理员权限 |

安装包 SHA-256：`827cf9b638f8265ebabb939fc53471e6c990ef99f54278bb21ebc51b50191b64`。完整更新说明见 [v5.7.0 正式发布页](https://github.com/AvroraCL/AIAS/releases/tag/v5.7.0)；应用内自动更新清单也已更新至 5.7.0。

v5.7.0 更新设置与日常操作：设置页改为清晰的分组列表，新增可保存的中文 / English 切换；统一调整浅色主题和侧栏图标。默认输出目录、跟随输入目录及同名文件策略现能正确作用于各处理功能；BLK 映射批量填充和烘焙缓存清理也增加了保护。

### 历史版本 v5.6.6

| 文件 | 说明 |
|---|---|
| [AIAS_5.6.6_x64-setup.exe](https://github.com/AvroraCL/AIAS/releases/download/v5.6.6/AIAS_5.6.6_x64-setup.exe) | 中文安装向导，按当前用户安装，无需管理员权限 |

v5.6.6 新增 BLK 生成：可扫描涂装 DDS 目录，在连线工作区查看真实 DXT5/RGBA8 缩略图、透明区域及一图多来源映射；C 文件支持批量选择指令，N 文件固定 `replace_tex`，导出前会核对规则并保护已有 BLK。

v5.6.5 侧栏导航更新：风格实验室 29 套风格归入 6 个可折叠分组（字符与线条、故障与信号、绘画与手作、像素与印刷、色彩与纹理、光学与扭曲），分组显示数量并高亮当前所在组；新增设置「切换区域时自动收起其他分类」（默认开启，可关闭以同时展开多组）。

v5.6.4 GPU 加速更新：新增 DirectML 后端（NVIDIA/AMD/Intel 全显卡通用，应用内一键下载约 12 MB，无需安装 CUDA Toolkit）；修复 CUDA 依赖缺失导致整批失败的问题（现自动回退 CPU 保证出图）；烘焙 UV 判定对齐 Substance Painter（镜像/分层堆叠与零面积退化视为设计，不合格源 UV 按原样烘焙并给出警告）；安装器语言按系统语言自动选择。

v5.6.3 模型烘焙更新：完善同模型 Mesh Map 工作流、智能 UV 与结果预览，预览保持模型源法线和硬边；烘焙过程持续显示阶段和进度，重新烘焙时保留上次结果，取消和失败不再破坏可用输出。

v5.6.2 安全加固：ORT CPU 运行库下载固化官方 SHA256；涂装删除/改名限定在涂装目录内。


| 文件 | 说明 |
|---|---|
| [AIAS_5.6.1_x64-setup.exe](https://github.com/AvroraCL/AIAS/releases/download/v5.6.1/AIAS_5.6.1_x64-setup.exe) | 中文安装向导，按当前用户安装，无需管理员权限 |

v5.6.1：修复「检查 UV」按钮失败、通知区界面冻结、涂装管理竞态等 20 项审计确认问题（详见 Release 说明）。

v5.5.18 重点修复：

- **修复模型导入崩溃（0xc0000409）的根因**：烘焙工作进程不再依赖系统 PATH 上的 libstdc++ 运行库 DLL——装有 Git 等工具的机器上曾被抢载不匹配版本，导致 xatlas 展开线程随机崩溃
- AI 抠图：「边界精修」「细节补全」阶段现在可以随时停止，NaN 输出直接报模型损坏而不是输出全透明图
- 烘焙 16 位法线/位置图错乱修复；UV 重叠大模型烘焙不再被看门狗误杀
- 材质列表勾选：按住左键滑动批量勾选，并修复单击翻转失效（5.5.18 遗留）
- 超分与抠图回退链的停止响应更及时（分块级取消）

v5.6.0 风格实验室大版本：

- 侧栏「风格化」与「风格实验室」合并，共 25 套风格（4 图像转风格 + 21 图片风格化）
- 新增故障家族 6 套：像素排序、坏块流动、信号撕裂、数据腐蚀、CRT 显像管、信号重影
- 数值自定义升级：数值直接键入、标签拖动微调、双击单参复位、滚轮步进、随机探索

历史版本与更新日志见 [Releases](https://github.com/AvroraCL/AIAS/releases)。

### 当前状态

AIAS 的贴图管线（PBR 合成 / 拆分、Mipmap、DDS 转换）与涂装管理已稳定可用，适合涂装作者日常使用。AI 抠图已接入 6 个离线模型并持续精修边缘质量——受推理分辨率限制，极细发丝与线稿边界仍有约 2-3 像素的精度上限，导出前建议先在对比视图中确认效果。模型烘焙（AO / 厚度 / 曲率 / 位置等 Mesh Map 一键离线烘焙）已进入可用状态：DXR 光线追踪加速、AI 降噪、SP 式按材质出图、自动 UV 展开带失败回退，详见下文「模型烘焙」。风格实验室已收录 25 套风格并在持续扩充，实时预览与导出在浏览器与桌面端均可使用。

### 模型烘焙

模型烘焙用于从当前静态模型生成 Substance Painter 等智能材质工作流所需的 Mesh Maps，不是高模向低模投射。源模型始终只读，烘焙不会覆盖原始 OBJ、MTL、GLB 或 glTF。

- **导入与 UV**：支持 OBJ、GLB 和 glTF；OBJ 缺少或部分缺少 MTL 时会保留 `usemtl` 材质槽继续导入。默认智能保留有效 UV，仅为缺失或越界的材质自动展开；也可选择全部重算或严格使用源 UV。UV 重叠（游戏涂装的镜像/分层堆叠）与零面积退化面视为无害设计，不触发重排或拦截。
- **输出贴图**：按材质生成 AO、切线法线、世界空间法线、曲率、位置、厚度、材质 ID 和 UV 检查图。AO 可使用 Intel OIDN 降噪；AO 与厚度可使用 DXR 1.1 光线追踪。输出仅含贴图与清单，供智能材质制作使用，不含模型。
- **工作区与预览**：全宽三维工作区提供模型、UV 和结果视图；烘焙期间持续显示当前阶段和进度。完成后保持当前视图，返回模型视图即可查看“着色 + AO”或指定结果贴图。
- **任务与结果**：重新烘焙时仍可查看和导出上次结果；新结果成功或形成有效部分输出后再切换。取消、失败或零文件任务不会删除原有可用结果。

### 系统要求

- Windows 10 或 Windows 11（x64）
- WebView2 Runtime（Windows 10/11 通常已经预装）
- AI 抠图 GPU 加速需要 NVIDIA 显卡（CUDA 12 / cuDNN 9，可在应用内一键安装）；无独显或显存不足时自动回退 CPU 推理，较慢但可用
- 模型烘焙的 AO / 厚度 GPU 加速需要支持 DXR 1.1 的显卡；其余几何 Mesh Map 由本地烘焙工作进程生成

正式版本已内嵌图像编解码与 ONNX Runtime；抠图模型体积较大（数百 MB 到 1 GB），首次使用时按需下载到应用数据目录。

### 安装与使用

从上方下载区或 [Releases](https://github.com/AvroraCL/AIAS/releases) 页面获取安装包。

启动后，典型的涂装制作流程是：

1. 添加需要处理的图片（PNG / JPG / TGA / DDS）。
2. 选择模式：PBR 合成、PBR 拆分、Mipmap、图片转 DDS、风格实验室、AI 抠图 / 超分、模型烘焙或涂装管理。
3. 确认参数并执行，结果写入指定目录，预览即时可见。
4. 用涂装管理把成品导入游戏的 UserSkins 目录。

AI 抠图首次选择某个模型时会自动下载；下载不可达时自动切换镜像重试。

### 已知限制

- 当前版本尚未进行代码签名，首次运行 Windows SmartScreen 可能显示安全提醒。
- AI 抠图在 512/1024 分辨率上推理，极细发丝与线稿边界存在约 2-3 像素的精度上限。
- AI 抠图与超分的 GPU 推理仅支持 NVIDIA 显卡；AMD / Intel 显卡使用 CPU 推理。
- AI 模型体积为数百 MB 到 1 GB，首次使用对应模型时需要下载；模型烘焙不下载或上传用户模型。

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
npm run build:fast   # 调试版打包（debug 二进制，含调试符号，不用于发布）
cd src-tauri && cargo test --release   # Rust 回归测试（无需模型与 GPU）
cargo test --manifest-path bake-worker/Cargo.toml   # 烘焙工作进程回归测试
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
├── index.html                     # 各模式界面骨架与侧栏导航
├── scripts/app.js                 # 模式切换、IPC 调用与预览逻辑
├── scripts/style-lab*.js          # 风格实验室：25 套风格化引擎（纯函数，node --test 可测）
├── scripts/model-bake.js          # 模型烘焙工作台（全屏 three.js / UV / 结果预览与任务状态）
└── styles/                        # 样式与性能模式样式
src-tauri/src/
├── main.rs                        # Tauri 命令、设置、纹理管线、涂装管理
├── anime.rs                       # AI 抠图：模型注册表、回退调度与公共入口
├── anime/
│   ├── runtime.rs                 # ORT DLL 获取、CUDA 运行时安装、会话缓存与显存回收
│   ├── infer.rs                   # ISNet / RTMDet+精修 / BiRefNet 系推理管线
│   ├── postprocess.rs             # 引导滤波、实心化、幽灵抑制、去污染、孤岛清理
│   └── tests.rs                   # 单元测试与 AB/GT 基准
├── superres.rs                    # AI 超分（分块推理 + 取消 + 进度）
└── safety.rs                      # 任务串行锁、协作式取消、原子写
bake-worker/                       # 烘焙工作进程（独立 exe，崩溃隔离）
├── src/bake.rs                    # AO / 曲率 / 位置等 Mesh Map 烘焙主循环
├── src/gpu.rs                     # DXR 光追（AO / 厚度）
├── src/model.rs                   # 模型加载、xatlas 自动 UV
├── vendor/xatlas/                 # xatlas 源码集成（FFI 异常隔离）
└── build.rs                       # 静态链 C++ 运行库，杜绝外来 DLL 抢载崩溃
tools/                             # 开发期质量工具：GT 对比评测、按轮次交付
```

依赖方向固定为 `main.rs → anime.rs → anime/*`：渲染层只经 IPC 与 Rust 通信；烘焙由独立工作进程 `bake-worker` 承担（主进程只转发事件），`tools/` 不参与应用构建，仅供开发期质量验证。

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
| 风格实验室 | 导入图片后切换 25 套风格并调参 | 预览实时刷新，数值键入/微调/随机探索生效，导出 PNG |
| 模型烘焙 | 导入缺少 MTL 或无有效 UV 的 OBJ，选择多种输出并重新烘焙/取消 | 自动修复 UV，Mesh Map 按材质输出；进度可见，上次结果保留 |
| 仅前端改动 | `npm run dev:renderer` | 浏览器中界面可交互（mock 数据），无需后端 |

### 参与开发

欢迎通过 Issue 提交问题、建议与复现步骤。改动必须保持「离线优先，一次装好」的定位：不引入在线服务或运行时依赖，图像处理与推理留在 Rust 侧，并保证 `cargo test` 与 `npm run check` 通过。

## AI 抠图模型来源与致谢

抠图功能使用的模型均来自以下社区开源项目，按需下载到本机离线运行，模型版权归各自作者所有，本项目不含任何训练产物、仅做格式转换与集成。

| 应用内名称 | 上游项目 |
|---|---|
| 动漫专精（AnimeSeg） | [nkta/birefnext-aniseg-ONNX](https://huggingface.co/nkta/birefnext-aniseg-ONNX)（基于 [BiRefNet](https://github.com/xikipedia/BiRefNet)） |
| 动漫特化（ToonOut） | [sprited/birefnet-toonout-onnx](https://huggingface.co/sprited/birefnet-toonout-onnx)（基于 [BiRefNet](https://github.com/xikipedia/BiRefNet)） |
| 高质量抠图（BiRefNet 1024） | [onnx-community/BiRefNet-ONNX](https://huggingface.co/onnx-community/BiRefNet-ONNX)（[BiRefNet 官方导出](https://github.com/xikipedia/BiRefNet)） |
| 轻量快速（BiRefNet Lite） | [BiRefNet-lite](https://github.com/xikipedia/BiRefNet-lite) |
| 动漫标准（ISNet） | [skytnt/anime-seg](https://huggingface.co/skytnt/anime-seg)（ISNet：[lerenhang/ISNetDIS](https://github.com/lerenhang/ISNetDIS)） |
| 动漫精细（RTMDet+精修） | [Faor-Mati/anime-character-segmentation](https://huggingface.co/Faor-Mati/anime-character-segmentation)（检测器基于 [RTMDet](https://github.com/open-mmlab/mmdetection)） |
| 发丝精修（ViTMatte） | [Xenova/vitmatte-small-distinctions-646](https://huggingface.co/Xenova/vitmatte-small-distinctions-646)（[ViTMatte](https://github.com/FoundationVision/ViTMatte)） |

推理引擎：[ONNX Runtime](https://github.com/microsoft/onnxruntime)（GPU 加速使用官方 onnxruntime-gpu 运行库）。感谢上述作者与 ONNX 社区的贡献。
