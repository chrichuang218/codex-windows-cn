# AGENTS.md

## 语言

默认使用简体中文回复和编写面向用户的说明。只有用户明确要求时再切换语言。

## 项目边界

本项目是面向中文 Windows 用户的 Codex Windows 中文助手，不是 OpenAI 官方项目，也不是 Codex 应用本体。

v1 只维护五条主路径：

- 安装
- 代理启动
- 检查更新 / 更新
- 卸载
- 启动器自更新

不要在修复问题时顺手加入诊断平台、网络加速、镜像源、代理配置等额外产品能力。

## 工作方式

修改前先读真实上下文和相关测试。优先做最小闭环修复，不做无关重构。

遇到启动、安装、更新、自更新、卸载卡住或失败时，先建立可验证反馈环：

- 优先加针对性测试。
- 其次用命令或最小复现脚本验证。
- 不要只靠猜测改 UI 文案掩盖根因。

错误处理要显式。不要吞异常、伪成功或隐藏关键失败。

## 构建和发布

发布包必须使用：

```powershell
.\scripts\package-release.ps1
```

不要用裸 `cargo build --release` 作为发布包；它可能生成仍指向 Vite 开发服务的 exe。

正式发布前至少运行：

```powershell
cargo fmt --check
cargo test
npm --prefix frontend test
npm --prefix frontend run build
.\scripts\package-release.ps1
```

发布产物为：

```text
dist/codex-launcher.exe
dist/codex-launcher.exe.sha256
```

GitHub Release 上传这两个文件即可。

## 版本号

发布新版时同步更新：

- `Cargo.toml`
- `Cargo.lock`
- `tauri.conf.json`
- `frontend/package.json`
- `frontend/package-lock.json`

tag 使用 `vX.Y.Z`，例如 `v0.1.4`。

## 启动器自更新注意点

启动器检查自身更新时优先使用 GitHub Releases 重定向：

```text
https://github.com/chrichuang218/codex-windows-cn/releases/latest
```

只有重定向路径失败时才回退到 GitHub API，避免未认证 API 限流。

自更新进度事件必须有后端状态兜底，前端应轮询最近事件，避免 Tauri event 丢失后 UI 卡在“准备中”。

GitHub 403 rate limit 要显示中文短提示，不要把英文底层错误和长 URL 直接暴露给用户。

## 代码风格

保持代码低复杂度、行为显式、命名清晰。优先早返回和平坦控制流。

只在解释意图、边界或取舍时写注释，不复述代码字面含义。

前端 UI 保持当前紧凑的桌面助手风格，不改成营销页或大段说明页。
