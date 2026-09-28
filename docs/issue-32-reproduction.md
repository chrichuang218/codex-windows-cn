# Issue #32：真实 MSIX 解压对照

验证日期：2026-09-28。修复前代码基线：`5e64ca59e43fbdf18a1dd2fb755237c40396bb74`。

## 结论

确认助手存在 MSIX 文件名未解码的缺陷。官方包中的 ZIP 条目使用 `%40oai`，助手原先直接将该名称写入磁盘；Node 查找 `@oai/sky` 时失败。修复后使用同一个官方包验证通过，无需创建目录链接或从 C 盘复制模块。

安装和更新分别在 `src/installer.rs` 中调用同一个 `extract_app`，本次修改覆盖两个入口。复现验证未修改本机正在运行的 Codex 安装目录或用户配置。本修复随助手 v0.1.16 发布；仅升级助手不会自动修复已经解压的 Codex 目录，需要由新版助手重新安装／解压或安装后续 Codex 更新。

## 安装包与测试目录

- 来源：项目现有 Microsoft Store Direct 下载链路，产品 `9PLM9XGG6VKS`。
- 包：`OpenAI.Codex_26.924.2738.0_x64__2p2nqsd0c76g0.msix`。
- 文件长度：`876623361` 字节。
- SHA256：`132431A26FE4DE44D1FA3C6E148506DBB4C8D806DAD40CFD48E3ED554F2755A2`。
- 测试根目录：`D:\Develop\codex-issue32-test`。
- 原包：`before\downloads\OpenAI.Codex_26.924.2738.0_x64__2p2nqsd0c76g0.msix`。
- 旧逻辑结果：`before\versions\26.924.2738.0`。
- 修复后结果：`after\versions\26.924.2738.0`。
- 微软官方解包结果：`official\app`，工具为 Windows SDK `10.0.26100.0` 的 x64 MakeAppx。
- 文件对照证据：`evidence\runtime-comparison.json`；官方解包日志：`evidence\makeappx.log`。

这些目录保留供复查，不是正在使用的安装目录。

## 实测结果

真实 ZIP 条目：`app/resources/cua_node/bin/node_modules/%40oai/sky/package.json`。

同包 `AppxBlockMap.xml` 记录的目标名称：`app\resources\cua_node\bin\node_modules\@oai\sky\package.json`。

| 检查 | 旧逻辑 | 修复后 | 微软 MakeAppx |
| --- | --- | --- | --- |
| `node_modules/@oai/sky/package.json` | 不存在 | 存在 | 存在 |
| `node_modules/%40oai/sky/package.json` | 存在 | 不存在 | 不存在 |
| 用包内 Node 导入 `@oai/sky` | `ERR_MODULE_NOT_FOUND` | 成功，`sky` 导出存在 | 成功 |
| 解析 `@oai/sky/service` | 主包导入已失败 | 成功 | 成功 |

修复后与官方解包的整个 `resources/cua_node` 对照：两侧各 **2366** 个文件，相对路径与逐文件 SHA256 全部相同。

微软 [AppxPackageObject::Unpack](https://github.com/microsoft/msix-packaging/blob/master/src/msix/unpack/AppxPackageObject.cpp) 在输出前调用 [Encoding::DecodeFileName](https://github.com/microsoft/msix-packaging/blob/master/src/msix/common/Encoding.cpp)，与本次官方工具实测一致。

## 修复与检查

- 在解压共用入口对 ZIP 相对路径做一次百分号解码，再进行路径安全检查。
- 使用已有依赖树中的 `percent-encoding`，避免手写解码器；保留字面 `+`，不重复解码 `%2540`。
- 解码后拒绝越界、绝对路径、NUL、冒号及尾随空格／点；同名文件拒绝覆盖。
- `cargo test --test extract_paths`：修复前 3 项失败，修复后 3 项通过。
- `cargo test --lib --tests`：通过。
- `cargo fmt --check`、`git diff --check`：通过。
- 回归测试包含两次向不同版本目录解压，以及 `$`、中文、百分号、路径越界和重名文件。

在新的测试目录复查修复后结果（已存在的版本目录会被验证程序拒绝）：

```powershell
cargo run --example check_msix_runtime -- D:\Develop\codex-issue32-test\verify D:\Develop\codex-issue32-test\before\downloads\OpenAI.Codex_26.924.2738.0_x64__2p2nqsd0c76g0.msix 26.924.2738.0
```

仅传测试目录时，验证程序会下载最新 Store 包，因此未来取得的版本可能不同。

## 与 Issue #32 的对应范围

[报告者最新补充](https://github.com/chrichuang218/codex-windows-cn/issues/32#issuecomment-5866760295)的截图为 `Module not found: @oai/sky`。本次真实包复现了这一模块加载失败，且证明助手的解压缺陷足以造成该症状及新版本再次缺少正确模块路径。

但报告者称 `@oai/sky` 与 `%40oai/sky` 均不存在，本次官方包则包含后者；其“最新”版本尚无准确版本号。本次也没有在隔离目录启动完整 Codex 会话，未复现最初的 Trusted RPC 报错、配置重写与 native pipe 重启恢复全过程。因此不能认定 #32 的所有表现均已复现或已被本补丁解决。

Windows 10 的 `SetIsBorderRequired` 截图接口问题独立于此修复，不在本次验证范围内。
