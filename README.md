# FoxHub

<p align="center"><strong>基于 Camoufox 内核的指纹浏览器与管理主控</strong></p>

FoxHub = **Camoufox 内核**（反检测 Firefox，本仓库 `browser/` 部分）+ **CamFox 桌面主控**（`gui/` 的 Tauri 应用）。它把 Camoufox 从「面向开发者的 Python 库」变成一个开箱即用的指纹浏览器工作站：图形化管理环境、一键多开、每个窗口独立指纹与数据。

> 上游项目：[daijro/camoufox](https://github.com/daijro/camoufox)（文档见 [camoufox.com](https://camoufox.com)）。本仓库在其基础上新增 `gui/` 桌面主控；浏览器构建体系保持上游原样。

---

## 它能做什么

- **环境（Profile）管理** —— 每个环境 = 独立浏览器进程 + 独立 Firefox 数据目录 + 独立指纹身份；分组、备注、搜索、批量导入导出、一键复制。
- **指纹生成** —— 操作系统 / 屏幕分辨率 / 窗口大小 / 语言 / 时区组合成一套连贯身份，由上游 `launch_options` 管线生成（fpgen 抽取 + 一致性校验 + 字体/语音/WebGL/媒体设备目录）。支持**固定指纹**（身份冻结在磁盘，每次打开完全一致，界面提供「指纹 ID」核对）与**每次启动随机**两种模式。
- **多开隔离** —— 同时打开任意数量的窗口；Cookie、存储、扩展、会话互不可见。
- **代理** —— 每环境独立 HTTP / SOCKS5 代理；带用户名密码的代理由内置本地转发器注入凭据（浏览器只连 `127.0.0.1`）。检测代理一键读取出口 IP 与 GeoIP 信息。
- **GeoIP 联动** —— 时区 / 语言 / 地理坐标按出口 IP 推导；WebRTC 声明跟随代理出口，避免泄露真实网络。
- **Cookie 导入导出** —— Playwright JSON 与 Netscape `cookies.txt` 两种格式，环境关闭时读写，跨环境迁移登录态。
- **自动化** —— 一键把环境挂到 Playwright WebSocket 端点，用 `firefox.connect()` 接管。
- **内核管理** —— 界面内下载 / 切换 / 删除浏览器内核版本。

界面提供中文 / 英文切换。

## 快速开始

### 方式一：直接使用安装包（推荐）

在 [Releases](https://github.com/UltraAIBrain/foxhub/releases) 下载 `CamFox_<版本>_x64-setup.exe` 安装。安装包自带指纹生成引擎（冻结的 Python sidecar），**无需安装 Python**；首次启动后在「内核管理」页下载浏览器内核（约 2 GB，一次性）即可使用。

### 方式二：从源码运行

前置条件：Node.js ≥ 18 + pnpm、Rust（Windows 为 msvc 工具链）、WebView2、**python.org 的 Python ≥ 3.11**（不要用 Microsoft Store 版，原因见 [gui/README.md](gui/README.md)）。

```bash
# 1. 安装内核生成依赖（一次）
pip install -e "./python[geoip]"
python -m camoufox fetch

# 2. 运行 GUI
cd gui
pnpm install
pnpm tauri dev
```

### 方式三：构建安装包

```bash
cd gui/sidecar
python -m PyInstaller camfox_sidecar.spec --noconfirm --distpath ../src-tauri/resources --workpath build
cd ..
pnpm tauri build      # 产物在 src-tauri/target/release/bundle/nsis/
```

## 目录结构

| 路径 | 内容 |
|---|---|
| `gui/` | **CamFox 桌面主控**：Tauri 2 + React/AntD 前端、Rust 启动与进程管理后端、Python sidecar（指纹生成）。文档：[gui/README.md](gui/README.md) |
| `browser/` | **Camoufox 内核**：获取上游 Firefox、应用补丁、构建反检测浏览器的完整体系（上游原样） |
| `python/` | `camoufox` Python 包（指纹生成管线与启动器，sidecar 复用它的实现） |
| `typescript/` | `camoufox` npm 包 |
| `ci/` | 测试流水线与既定决策（`ci/tribal-rules.yml`） |
| `docs/` | 设计笔记：补丁升级指南、宿主一致性、媒体设备等 |

## 测试

```bash
cd gui/src-tauri && cargo test       # Rust：存储、环境变量分块、代理转发器、进程识别
cd gui/sidecar  && pytest tests -q   # sidecar：协议、身份摘要、WebRTC 映射
cd gui          && npx tsc --noEmit  # 前端类型
```

浏览器构建与反检测行为的上游测试见 `browser/tests/` 与 `ci/`。

## 已知边界

- 指纹生成依赖 `camoufox` 包的数据文件（字体、语音、GPU 目录），开发机需要安装它；打包版已内置。
- Windows 的启动器进程行为、温和关闭的会话落盘、指纹窗口的首次闪现等运行细节，见 [gui/README.md](gui/README.md) 的「已知限制」与「常见问题」。
- 反检测能力由 Camoufox 内核提供（C++/Juggler 层实现，页面不可见），本仓库不修改其实现。

## 致谢

- [daijro/camoufox](https://github.com/daijro/camoufox) 与 [scrapfly/fingerprint-generator](https://github.com/scrapfly/fingerprint-generator) —— 本项目的内核与指纹管线基础。
