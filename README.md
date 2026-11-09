# FoxHub

<p align="center"><strong>指纹浏览器与管理主控</strong></p>

FoxHub 是一个开箱即用的指纹浏览器工作站：图形化管理浏览器环境、一键多开，每个窗口拥有独立指纹与完全隔离的数据。面向网页抓取、多账号运营与自动化测试。

- **环境（Profile）管理** —— 每个环境 = 独立浏览器进程 + 独立数据目录 + 独立指纹身份；分组、备注、搜索、批量导入导出、一键复制。
- **指纹生成** —— 操作系统 / 分辨率 / 窗口 / 语言 / 时区组合成一套连贯身份，由内置的指纹抽取与一致性校验管线生成。支持**固定指纹**（身份冻结，每次打开完全一致，界面提供「指纹 ID」核对）与**每次启动随机**两种模式。
- **多开隔离** —— 同时打开任意数量的窗口；Cookie、存储、扩展、会话互不可见。
- **代理** —— 每环境独立 HTTP / SOCKS5 代理；带凭据的代理由内置本地转发器注入（浏览器只连 `127.0.0.1`）。一键检测出口 IP 与 GeoIP。
- **GeoIP 联动** —— 时区 / 语言 / 地理坐标按出口 IP 推导；WebRTC 声明跟随代理出口，避免泄露真实网络。
- **Cookie 导入导出** —— Playwright JSON 与 Netscape `cookies.txt` 两种格式，跨环境迁移登录态。
- **自动化** —— 一键把环境挂到 Playwright WebSocket 端点，用 `firefox.connect()` 接管。
- **内核管理** —— 界面内下载 / 切换 / 删除浏览器内核版本。

界面提供中文 / 英文切换。

## 快速开始

### 方式一：直接使用安装包（推荐）

在 [Releases](https://github.com/UltraAIBrain/foxhub/releases) 下载 `CamFox_<版本>_x64-setup.exe` 安装。安装包自带指纹生成引擎，**无需安装 Python**；首次启动后在「内核管理」页下载浏览器内核（约 2 GB，一次性）即可使用。

### 方式二：从源码运行

前置条件：Node.js ≥ 18 + pnpm、Rust（Windows 为 msvc 工具链）、WebView2、**python.org 的 Python ≥ 3.11**（不要用 Microsoft Store 版，原因见 [gui/README.md](gui/README.md)）。

```bash
# 1. 安装依赖（一次；camoufox 为上游引擎包的命令名，保留原名）
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
| `gui/` | **桌面主控**：Tauri 2 + React/AntD 界面、Rust 启动与进程管理后端、Python 指纹生成 sidecar。文档：[gui/README.md](gui/README.md) |
| `browser/` | 浏览器内核构建体系：获取 Firefox 源码、应用反检测补丁、产出内核 |
| `python/` | 指纹生成管线与启动器 Python 包（sidecar 复用其实现） |
| `typescript/` | 对应的 npm 包 |
| `ci/` | 测试流水线与既定决策 |
| `docs/` | 设计笔记：补丁升级指南、宿主一致性、媒体设备等 |

## 测试

```bash
cd gui/src-tauri && cargo test       # Rust：存储、环境变量分块、代理转发器、进程识别
cd gui/sidecar  && pytest tests -q   # sidecar：协议、身份摘要、WebRTC 映射
cd gui          && npx tsc --noEmit  # 前端类型
```

## 许可与开源参考

本项目基于开源社区成果构建，内核与指纹引擎源自 **[Camoufox](https://github.com/daijro/camoufox)**（及 [fpgen](https://github.com/scrapfly/fingerprint-generator)），在此致谢——上游是本项目得以存在的开源参考；浏览器构建体系沿用其架构，相关许可（MPL/GPL）随源码继承。桌面主控部分（`gui/`）为本仓库新增。
