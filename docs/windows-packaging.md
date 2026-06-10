# BeeBotOS Windows 打包指南

本文档说明如何使用仓库根目录的 `beebotos-dev.ps1` 生成 Windows 发布目录和 zip 包，并给 Inno Setup 等安装包工具使用。

## 适用场景

- Windows 本机打包：在 Windows PowerShell / PowerShell 7 中运行脚本。
- Linux 交叉打包 Windows 产物：安装 `pwsh`、Windows Rust target 和交叉编译工具链后运行脚本。

`beebotos-launcher.exe` 只在 Windows 本机打包路径生成；Linux 交叉打包会跳过 Launcher。

脚本会生成：

```text
dist/beebotos/
dist/beebotos-<target>.zip
```

Inno Setup 的 `SourceRoot` 应指向 `dist/beebotos` 目录，而不是源码根目录。

## 前置条件

Windows 本机打包需要：

- Rust toolchain
- PowerShell
- `trunk`
- 项目依赖可正常编译

Linux 交叉打包 Windows 产物通常需要：

- `pwsh`
- Rust target：`x86_64-pc-windows-gnu`
- MinGW 交叉工具链
- `trunk`

安装 Rust target：

```bash
rustup target add x86_64-pc-windows-gnu
```

## 打包命令

在仓库根目录执行：

```powershell
.\beebotos-dev.ps1 pack all
```

在 Linux 上用 PowerShell 交叉打包：

```bash
pwsh ./beebotos-dev.ps1 pack all
```

如果需要显式指定 Windows target：

```bash
BEEBOTOS_PACKAGE_TARGET=x86_64-pc-windows-gnu pwsh ./beebotos-dev.ps1 pack all
```

`pack all` 会编译并打包：

- `beebotos-launcher.exe`
- `beebotos-gateway.exe`
- `web-server.exe`
- `beehub.exe`
- Web 前端静态文件
- 配置、数据库迁移、内置 skills、内置 workflows 和运行脚本

## 发布目录必须包含的内容

打包后的 `dist/beebotos` 至少应包含：

```text
beebotos/
├── beebotos-launcher.exe
├── beebotos-gateway.exe
├── web-server.exe
├── beehub.exe
├── beebotos-run.ps1
├── config/
│   ├── beebotos.toml
│   └── web-server.toml
├── migrations_sqlite/
│   ├── 001_initial.sql
│   └── ...
├── skills/
│   ├── daily/
│   ├── coding/
│   ├── productivity/
│   └── ...
├── workflows/
│   ├── content_factory.yaml
│   ├── xauusd_hourly.yaml
│   └── ...
├── index.html
├── beebotos-web-*.js
├── beebotos-web-*_bg.wasm
├── public/
└── style/
```

这些目录/文件的作用：

| 路径 | 用途 |
| --- | --- |
| `beebotos-launcher.exe` | 用户机默认入口，用于配置 `.env`、启停服务、打开 Web 和查看日志 |
| `beebotos-gateway.exe` | Gateway 主服务，负责 API、Agent runtime、SkillRegistry 等 |
| `web-server.exe` | 本地 Web 静态文件服务器，默认端口 `8090` |
| `beehub.exe` | BeeHub 服务产物 |
| `beebotos-run.ps1` | Windows 生产启动/停止/状态脚本 |
| `config/` | Gateway 和 Web Server 配置 |
| `migrations_sqlite/` | SQLite 数据库迁移，缺失会导致数据库初始化失败 |
| `skills/` | 内置默认 Markdown skills，启动时会加载到 SkillRegistry |
| `workflows/` | 内置项目级工作流定义，启动时会加载到 WorkflowRegistry，并显示在 Web 工作流页面 |
| `index.html`、`*.js`、`*.wasm`、`style/`、`public/` | Web 前端静态资源 |

`config/web-server.toml` 在打包时会被改成：

```toml
[static_file]
path = "."
```

这表示安装后 `web-server.exe` 会从应用安装目录直接提供 Web 静态资源。

## Launcher 和密钥配置

Windows 用户机默认打开 `beebotos-launcher.exe`。Launcher 会在安装目录维护 `.env`：

```env
DOUBAO_API_KEY=
IMAGE_GENERATION_API_KEY=
VIDEO_GENERATION_API_KEY=
BEE_ALLOW_NETWORK=1
```

大模型、图片生成、视频生成的密钥只写入 `.env`。`config/beebotos.toml` 保留模型名、`base_url`、`temperature` 等非敏感配置。

## 内置 skills 和市场安装 skills

内置默认 skills 位于发布目录：

```text
{app}/skills
```

Gateway 启动时会通过 `SkillRegistry` 加载这些内置 skills。不是只要目录存在就直接调用，而是需要启动时扫描并注册。

市场安装的 skills 默认位于：

```text
{app}/data/skills
```

也可以通过环境变量覆盖：

```powershell
$env:BEEBOTOS_SKILLS_DIR = "D:\BeeBotOSData\skills"
```

注意：`data/workspace` 是 exec 工具的工作目录，不是当前代码里的市场 skill 安装目录。

当前启动流程中，Gateway 会加载磁盘安装 skills 和内置 skills。如果用户安装的 skill 与内置 skill 使用相同 ID，运行时 registry 可能发生同名覆盖。因此建议：

- 市场 skill 使用唯一 ID。
- 后续如需支持用户安装 skill 覆盖内置 skill，应保证启动顺序为先加载内置 `skills/`，再加载 `data/skills/`。

## 内置 workflows 和用户安装 workflows

内置项目级 workflows 位于发布目录：

```text
{app}/workflows
```

Gateway 启动时会通过 `WorkflowRegistry` 加载这些内置 workflows，并在 Web 工作流页面展示。

兼容旧版本的工作流目录仍然保留：

```text
{app}/data/workflows
{app}/data/workflows/local
```

推荐加载优先级为：

```text
workflows -> data/workflows -> data/workflows/local
```

也就是说，如果多个目录中存在相同 workflow ID，后加载目录会覆盖前加载目录。Web 页面安装的 workflow 默认进入 `data/workflows/local`，可以覆盖内置 workflow。

## Inno Setup 使用方式

项目已提供完整的 Inno Setup 脚本：`tools/scripts/setup/beebotos-setup.iss`。

### 快速开始

1. 打包发布目录：
   ```powershell
   .\beebotos-dev.ps1 pack all
   ```
2. 将 `dist\beebotos` 复制到 Windows 上的 staging 目录，例如：
   ```
   C:\Users\you\Desktop\beebotos_installer\beebotos
   ```
3. 编辑 `tools/scripts/setup/beebotos-setup.iss`，修改 `SourceRoot`：
   ```iss
   #define SourceRoot "C:\Users\you\Desktop\beebotos_installer\beebotos"
   ```
4. 用 Inno Setup Compiler (`iscmplr.exe`) 编译脚本，生成安装程序。

### 脚本特性

- **自动创建数据目录**：安装时自动创建 `{app}\data`、`{app}\data\run`、`{app}\data\logs`、`{app}\data\skills`、`{app}\data\workspace`。
- **启动菜单项**：包含 Launcher、停止、查看状态、打开 Web 四个快捷方式。
- **卸载前自动停止服务**：通过 `beebotos-run.ps1 stop all` 确保进程被正确终止。
- **🛡️ 卸载保留用户数据库**：详见下方「卸载时保留数据库」章节。

### 手动编写脚本参考

如果你需要自定义脚本，以下是核心段落的参考：

`SourceRoot` 指向打包后的发布目录：

```iss
#define SourceRoot "C:\Users\you\Desktop\beebotos_installer\beebotos"
```

文件复制规则递归复制完整发布目录：

```iss
[Files]
Source: "{#SourceRoot}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
```

安装时创建运行数据目录：

```iss
[Dirs]
Name: "{app}\data"
Name: "{app}\data\run"
Name: "{app}\data\logs"
Name: "{app}\data\skills"
Name: "{app}\data\workspace"
```

启动菜单项：

```iss
[Icons]
Name: "{autoprograms}\BeeBotOS\BeeBotOS Launcher"; Filename: "{app}\beebotos-launcher.exe"; WorkingDir: "{app}"
Name: "{autoprograms}\BeeBotOS\停止 BeeBotOS"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\beebotos-run.ps1"" stop all"; WorkingDir: "{app}"
Name: "{autoprograms}\BeeBotOS\查看状态"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -NoExit -File ""{app}\beebotos-run.ps1"" status"; WorkingDir: "{app}"
Name: "{autoprograms}\BeeBotOS\BeeBotOS Web"; Filename: "http://localhost:8090"
```

卸载时先停止服务：

```iss
[UninstallRun]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\beebotos-run.ps1"" stop all"; WorkingDir: "{app}"; Flags: runhidden
```

## 卸载时保留数据库

BeeBotOS 的运行时数据库文件位于安装目录的 `data\` 子目录下：

| 文件 | 说明 |
|------|------|
| `data\beebotos.db` | 主数据库（聊天记录、Agent、会话、配置等） |
| `data\beebotos.db-wal` | SQLite WAL（Write-Ahead Log） |
| `data\beebotos.db-shm` | SQLite SHM（共享内存） |
| `data\memory_search.db` | 记忆搜索索引数据库 |
| `data\memory_search.db-wal` | SQLite WAL |
| `data\memory_search.db-shm` | SQLite SHM |

**这些文件必须在卸载时保留**，否则用户会丢失全部聊天记录、Agent 配置和记忆数据。

### 实现原理

`tools/scripts/setup/beebotos-setup.iss` 使用 Inno Setup 的 `[Code]` 段实现数据库保护：

1. **卸载开始前** (`InitializeUninstall`)：
   - 将 `data\` 下的 6 个数据库文件复制到 `%TEMP%\BeeBotOS_DB_Backup_<时间戳>\`
2. **标准卸载流程**：
   - Inno Setup 正常删除所有安装文件
3. **卸载完成后** (`CurUninstallStepChanged` → `usPostUninstall`)：
   - 从备份目录将数据库文件恢复回 `{app}\data\`
   - 清理临时备份目录

### 注意事项

- 此机制依赖 `beebotos-run.ps1 stop all` 在卸载前正确停止 Gateway 进程，确保数据库文件未被占用。
- 如果用户手动删除安装目录（不通过 `unins000.exe`），数据库文件会一并丢失——这是预期行为，因为数据文件当前与程序安装在同一目录。
- 后续如需将数据库迁移到独立的用户数据目录（如 `%LOCALAPPDATA%\BeeBotOS`），可进一步改进。

## 打包后校验

在 Linux/macOS 上可用：

```bash
find dist/beebotos -maxdepth 2 -type f | sort | head -80
unzip -l dist/beebotos-*.zip 'beebotos/*.exe' 'beebotos/skills/*' 'beebotos/workflows/*' | head -100
```

在 Windows PowerShell 中可用：

```powershell
Get-ChildItem -Recurse dist\beebotos | Select-Object FullName
Expand-Archive dist\beebotos-*.zip -DestinationPath dist\check -Force
Get-ChildItem -Recurse dist\check\beebotos\skills | Select-Object FullName
Get-ChildItem -Recurse dist\check\beebotos\workflows | Select-Object FullName
```

必须确认：

- Windows 本机打包时，`dist/beebotos` 下有 `beebotos-launcher.exe` 和三个服务 `.exe`。
- `dist/beebotos/skills` 存在并包含默认 skill 文件。
- `dist/beebotos/workflows` 存在并包含默认 workflow 文件。
- `dist/beebotos/migrations_sqlite` 存在。
- `dist/beebotos/config/web-server.toml` 中 `path = "."`。
- zip 解压后顶层目录是 `beebotos/`，且内容与 `dist/beebotos` 一致。

## 常见问题

### Windows 上 Node/Python 在 PowerShell 可用，但 BeeBotOS exec 找不到

确认使用的是最新打包的 `beebotos-gateway.exe` 和 `beebotos-run.ps1`。运行脚本会刷新系统和用户 PATH，Gateway 的 exec 工具也会读取 Windows 注册表中的 Machine/User PATH。

安装 Node/Python 后，如果 BeeBotOS 已经在运行，先停止再启动：

```powershell
.\beebotos-run.ps1 stop all
.\beebotos-run.ps1 start all
```

### PowerShell 输出乱码或出现版权 banner

确认使用最新 `beebotos-gateway.exe`。Windows exec 路径应使用 `-NoLogo -NoProfile -NonInteractive -EncodedCommand`，并设置 UTF-8 输入/输出编码。

如果仍然看到旧行为，优先检查安装目录是否还是旧 exe。

### 安装后 Web 页面空白

检查：

- `index.html` 是否在 `{app}` 根目录。
- `beebotos-web-*.js` 和 `beebotos-web-*_bg.wasm` 是否在 `{app}` 根目录。
- `config/web-server.toml` 是否为 `path = "."`。
- `web-server.exe` 是否成功启动并监听 `8090`。

### 缺少内置 skills

检查发布目录和 zip：

```bash
find dist/beebotos/skills -type f | wc -l
unzip -l dist/beebotos-*.zip 'beebotos/skills/*' | head
```

如果没有 `skills/`，说明打包脚本或手工复制流程不完整。应重新运行：

```powershell
.\beebotos-dev.ps1 pack all
```

### 缺少内置 workflows

检查发布目录和 zip：

```bash
find dist/beebotos/workflows -type f | wc -l
unzip -l dist/beebotos-*.zip 'beebotos/workflows/*' | head
```

如果没有 `workflows/`，说明打包脚本或手工复制流程不完整。应重新运行：

```powershell
.\beebotos-dev.ps1 pack all
```
