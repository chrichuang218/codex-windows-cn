# Codex Windows 中文助手 v1 Smoke 验收

本记录用于发布前重复验证 v1 的 **五条主路径**。v1 边界是中文安装更新助手，不包含诊断报告、一键修复、网络检测、代理配置或镜像分发。

## 自动验证

每次发布前运行：

```powershell
cargo test --all-targets
npm --prefix frontend test
npm --prefix frontend run build
cargo tauri build --no-bundle
cargo build
```

## 主路径验收

| 主路径 | 可重复验证方式 |
| --- | --- |
| 安装 | `tests/install_bridge.rs` 覆盖安装默认值、安装请求到 `InstallOptions` 的桥接，以及安装 worker 事件中文化；React 测试覆盖开始安装与进度展示。 |
| 代理启动 | `tests/proxy_bridge.rs` 覆盖可启动路径和无 `Codex.exe` 的中文失败状态；React 测试覆盖启动按钮和启动结果。 |
| 检查更新/更新 | `tests/update_bridge.rs` 覆盖更新状态、延后/跳过动作和更新事件中文化；React 测试覆盖立即更新、稍后、跳过、关闭提醒和完成事件。 |
| 卸载 | `tests/uninstall_bridge.rs` 覆盖卸载确认、安装签名安全阻断和卸载事件中文化；React 测试覆盖删除/保留清单、确认卸载和完成事件。 |
| 自更新 | `tests/launcher_update_bridge.rs` 覆盖 `chrichuang218/codex-windows-cn` release 源、自更新状态、提醒动作和下载/校验/替换事件中文化；React 测试覆盖发布页入口、应用更新和完成事件。 |

## 发布产物

Release workflow 应产出：

- `codex-launcher.exe`
- `codex-launcher.exe.sha256`

发布说明必须包含：

- 不是 OpenAI 官方项目
- 不修改、不重新分发 Codex 本体
- SHA256 校验方式
- GitHub Actions 构建来源验证
