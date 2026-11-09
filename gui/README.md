# CamFox 桌面主控

FoxHub 的图形主控端：基于 Tauri 2 的指纹浏览器管理程序，仿 AdsPower / HubStudio 的使用方式，管理并多开指纹浏览器环境。

```
┌────────────────────────┐
│  gui/src  (React+AntD) │  界面：环境管理 / 内核管理 / 设置，中英双语
├────────────────────────┤
│  gui/src-tauri (Rust)  │  环境存储、启动器、进程监管、代理转发器、Cookie 读写
├────────────────────────┤
│  gui/sidecar (Python)  │  指纹生成 —— 包装上游 launch_options 管线
├────────────────────────┤
│  浏览器内核            │  身份经 CAMOU_CONFIG_* 环境变量注入
└────────────────────────┘
```

**指纹逻辑只在 Python 一处实现。** Rust 后端从不自行拼装身份：它通过一次性 sidecar 进程（`prepare` 命令）调用上游的 `launch_options` 管线（fpgen 抽取、一致性校验、字体/语音/WebGL 目录），拿到完整身份后，以 `CAMOU_CONFIG_1..N` / `CAMOU_PREFS_1..N` 环境变量启动浏览器——与官方 Python 启动器相同的注入通道，但不依赖 Playwright。

## 功能一览

- **环境管理** —— 创建 / 编辑 / 复制 / 删除 / 搜索 / 分组；批量导入导出（JSON）；批量启动与关闭。
- **指纹** —— 每环境固定或每次启动随机；编辑面板预填主机默认值，生成后展示完整身份（UA、平台、屏幕、色深、显卡、字体/语音数、媒体设备、时区/语言）与**指纹 ID**（冻结身份的哈希，跨启动核对一致）。修改影响身份的参数会自动重新生成。
- **多开隔离** —— 每环境独立进程 + 独立数据目录 + 独立身份；真实窗口尺寸同步为身份声明的外窗大小。
- **代理** —— HTTP / SOCKS5，带凭据的经内置本地 SOCKS5 转发器注入；一键检测出口 IP 与 GeoIP。
- **GeoIP** —— 按出口 IP 推导时区/语言/坐标；不填代理时用本机公网 IP。
- **WebRTC** —— 三种模式：跟随身份（配合 GeoIP 声明代理出口 IP）、完全屏蔽、指定 IPv4（同时关闭 IPv6 候选）。
- **Cookie** —— Playwright JSON / Netscape cookies.txt 导入导出（环境关闭时）。
- **自动化** —— 一键暴露 Playwright WebSocket 端点（`firefox.connect()` 接管；非持久会话）。
- **内核管理** —— 下载 / 切换 / 删除内核版本，下载进度实时显示。
- **进程监管** —— 关闭走 WM_CLOSE 温和退出（Cookie/会话落盘），20 秒后强制；GUI 重启后自动重新接管仍在运行的浏览器；出口时按设置批量关闭。

## 前置条件

- Node.js ≥ 18 与 pnpm
- Rust（Windows 为 msvc 工具链）与 WebView2
- **python.org 的 Python ≥ 3.11**（开发运行需要；打包版内置）
- 引擎包与 GeoIP 扩展（`camoufox` 为上游引擎包名，保留原名）：
  ```bash
  pip install -e "./python[geoip]"
  python -m camoufox fetch
  ```
- 浏览器内核：也可在应用内「内核管理」页下载

> **不要使用 Microsoft Store 版 Python。** 它对 `AppData` 的写入走按应用虚拟化，装出来的内核文件对所有其他进程不可见，Playwright 驱动与 GUI 启动器都会报"executable doesn't exist"。

## 开发运行

```bash
cd gui
pnpm install
pnpm tauri dev
```

开发版自动在 `gui/sidecar` 找到 sidecar 源码。打包版则优先使用冻结的 sidecar（见下）。

## 打包（自带 Python 的安装包）

```bash
cd gui/sidecar
python -m PyInstaller camfox_sidecar.spec --noconfirm --distpath ../src-tauri/resources --workpath build
cd ..
pnpm tauri build
```

产物：`src-tauri/target/release/bundle/nsis/CamFox_<版本>_x64-setup.exe`。安装后布局：`<安装目录>\{camfox-gui.exe, sidecar\{camfox-sidecar.exe, _internal\}}`。

`src-tauri/resources/camfox-sidecar/.gitkeep` 保证冻结产物缺失时开发构建仍然可用；冻结产物不入库。**改过引擎包或 sidecar 后必须重建冻结包**，否则安装包会带旧指纹逻辑。

## 测试

```bash
cd gui/src-tauri && cargo test        # 存储、env 分块、转发器握手、进程识别、xulstore
cd gui/sidecar  && pytest tests -q    # 协议、身份摘要、WebRTC 映射、GPU 缓存
cd gui          && npx tsc --noEmit   # 前端类型
```

端到端手工验收（对照清单）：

1. 建两个不同 OS 设置的环境并同时打开——两个独立窗口，UA / 平台 / 屏幕各不相同。
2. 在环境 A 登录某站点，环境 B 不共享该会话。
3. 给环境配带认证的 SOCKS5 代理，「检测代理」读出的出口 IP 应为代理 IP。
4. 从 A 导出 Cookie（JSON）导入新环境，登录态应迁移。
5. 打开自动化端点，用 `p.firefox.connect("ws://127.0.0.1:<port>")` 连接并读页面标题。
6. 同一环境反复打开，「指纹 ID」应完全一致；canvas 像素逐位一致。

## 常见问题

| 现象 | 原因与处理 |
|---|---|
| 启动报 `executable doesn't exist` | 用了 Microsoft Store Python（AppData 虚拟化）。装 python.org 版并在设置页指定解释器；内核用新解释器重新下载。 |
| 首次生成指纹时闪出浏览器窗口 | 正常行为：启动器须以与真实启动相同的显示模式探测主机 GPU（否则 canvas 声明不一致）。结果落盘缓存（换内核失效，30 天 TTL），之后不再出现。 |
| 关闭窗口后刚登录的 Cookie 不在 | 旧版本的强制杀进程问题，已改为 WM_CLOSE 温和关闭 + 20 秒宽限。请更新到最新构建。 |
| PNG 导出（toDataURL）每次会话不同 | 与原版 Firefox 一致的每会话随机 `deBG` 块，像素本身固定（原版 Firefox 同样如此）。只影响对整段 PNG 字符串取哈希的站点。 |
| 重启 GUI 后浏览器还在跑但列表没有 | 新版会自动重新接管；若没有，确认该环境数据目录未被移动。接管后的认证代理需重开一次环境（转发器是进程内的）。 |

## 已知限制

- 开发运行需要本机 Python + 引擎包；打包版内置冻结 sidecar，无此依赖。
- 固定身份跨机器使用时，canvas 走宿主一致性路径的失效分支（随机回读）——与官方启动器行为一致；身份声明的 GPU 与宿主渲染设备一致时像素才是固定的。
- 自动化模式为非持久会话（Playwright launchServer 无法通过 WebSocket 暴露持久 context），Cookie 不落盘；需要持久登录态用普通打开 + Cookie 导出。
- 浏览器裸启动无法原生使用带认证代理，由内置转发器代劳；GUI 重启无法恢复转发器，需重开环境。

## 开源参考

指纹生成与浏览器构建基于开源社区成果：内核与管线源自 **[Camoufox](https://github.com/daijro/camoufox)**（[fpgen](https://github.com/scrapfly/fingerprint-generator)），相关许可随源码继承；桌面主控（`gui/`）为本仓库新增。
