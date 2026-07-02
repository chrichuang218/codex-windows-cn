# Codex Windows 中文助手

面向中文 Windows 用户的 **中文安装更新助手**。v1 只稳定交付 **五条主路径**：安装、代理启动、检查更新/更新、卸载、自更新。

本项目不是 OpenAI 官方项目，也不是 Codex 应用本体。Codex、OpenAI 及相关标识归其权利方所有。

## 项目边界

- 不修改、不重新分发 Codex 本体。
- 不打包第三方镜像源，不替换官方分发链路。
- 不在 v1 增加诊断报告、一键修复、网络检测、代理配置或镜像分发。
- 只提供一个中文 Windows 启动器，用来下载官方 Microsoft Store MSIX、安装到本机、启动已安装的 Codex、检查更新、卸载本工具管理的文件，并更新启动器自身。

## 五条主路径

| 主路径 | v1 行为 |
| --- | --- |
| 安装 | 选择安装模式、安装位置和基础选项，下载官方 Microsoft Store MSIX，解压到版本目录，写入启动器配置。 |
| 代理启动 | 从已安装版本中解析最新 `Codex.exe`，保持稳定入口，并启动 Codex。 |
| 检查更新/更新 | 检查官方 Microsoft Store MSIX 最新版本，支持立即更新、稍后提醒、跳过版本和关闭提醒。 |
| 卸载 | 展示将删除和将保留的内容，只删除启动器管理的版本、缓存、配置、快捷方式和卸载入口。 |
| 自更新 | 检查 `chrichuang218/codex-windows-cn` 的 GitHub Release，下载 `codex-launcher.exe`，校验 SHA256，自检后替换启动器。 |

## 下载与验证

最新发布页：

- `codex-launcher.exe`: <https://github.com/chrichuang218/codex-windows-cn/releases/latest/download/codex-launcher.exe>
- `codex-launcher.exe.sha256`: <https://github.com/chrichuang218/codex-windows-cn/releases/latest/download/codex-launcher.exe.sha256>

基础完整性校验：

```powershell
(Get-FileHash .\codex-launcher.exe -Algorithm SHA256).Hash
# 与 codex-launcher.exe.sha256 中的 SHA256 值对比
```

构建来源验证：

```powershell
gh attestation verify codex-launcher.exe --owner chrichuang218
```

Release 产物由 GitHub Actions 构建，并发布 SHA256 文件。校验通过只能证明你拿到的是本仓库对应构建产物，不能表示它是 OpenAI 官方发布物。

## 官方 MSIX 来源

安装和 Codex 更新路径使用官方 Microsoft Store MSIX：

- 默认直连 Microsoft Store 相关接口解析和下载。
- 可用 `winget` 作为备用下载方式。
- 可使用本地 MSIX 作为手动兜底，但文件仍应来自可信官方渠道。

启动器只解压并管理本机安装目录，不修改 Codex 应用包内容。

## 安装布局

默认安装后目录大致如下：

```text
<root>/
├── codex-launcher.exe
├── updater.json
├── versions/
│   ├── 26.422.2437.0/
│   ├── 26.500.0.0/
│   └── current -> 26.500.0.0
└── downloads/
```

`updater.json` 保存安装模式、当前版本、更新策略、保留版本数、自更新提醒状态等运行时配置。

## 从源码构建

需要 Windows、Rust/MSVC 工具链，以及前端依赖：

```powershell
npm --prefix frontend install
npm --prefix frontend run build
cargo build --release
```

输出文件位于：

```text
target/release/codex-launcher.exe
```

常用验证：

```powershell
cargo test --all-targets
npm --prefix frontend test
npm --prefix frontend run build
cargo build
```

## 卸载说明

卸载流程会先校验安装目录是否属于本工具管理，避免误删桌面、下载目录、用户目录、Program Files 根目录或磁盘根目录。

卸载会删除：

- 已安装的 Codex 版本目录
- 下载缓存
- 启动器配置
- 开始菜单快捷方式
- Windows 卸载入口

卸载会保留：

- Codex 登录数据
- 日志和诊断信息
- 安装目录中非本工具创建的其他文件

## 许可证

本仓库代码使用 MIT License。第三方依赖遵循其各自许可证。

再次强调：本项目是社区开源的 Codex Windows 中文助手，不是 OpenAI 官方项目；它不修改、不重新分发 Codex 本体。
