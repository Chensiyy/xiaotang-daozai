# 插件 SDK 开放任务（第 1–4 步）

第 0 步「能力表」已经完成，在分支 `feat/plugin-capability-table`（提交 4e4deec）。准入规则改成了 [spike/capabilities.rs](../../../spike/capabilities.rs) 里的一张表。`owner` 只决定插件设置放在哪个设置页、哪个模块关掉时整个插件暂停。一个脚本插件可以同时声明多种输出，每种输出只跟自己所在模块的开关走。另外新增了 `calendar.marker`。

下面四个任务都从这个分支开始，彼此独立，可以同时做。

| 任务 | 分支 | 工作树 |
| --- | --- | --- |
| [第 1 步：按钮、表单、事件与提醒](1-actions.md) | `feat/plugin-actions` | `01-专项工程\插件按钮与事件` |
| [第 2 步：联网取数与凭据](2-https-source.md) | `feat/plugin-https-source` | `01-专项工程\插件联网取数` |
| [第 3 步：状态页组件与主题令牌](3-status-components.md) | `feat/plugin-status-components` | `01-专项工程\插件状态页组件` |
| [第 4 步：皮肤、表情、图标资源包](4-asset-packs.md) | `feat/plugin-asset-packs` | `01-专项工程\插件资源包` |

开工命令，以第 1 步为例：

```powershell
git worktree add -b feat/plugin-actions "D:\A-TangHao\REA\Windows小工具\01-专项工程\插件按钮与事件" feat/plugin-capability-table
```

## 已经定下的方向

- 作者用岛仔提供的组件描述界面，用少量主题令牌调样式，最后由岛仔绘制。不做自由绘制，不嵌网页。
- 不做发布者签名。插件照旧在插件仓审 PR，由小唐本人过目后发布。
- 不引入付费服务或付费证书。

## 共同规则

- 所有回复用中文。
- 本机 Cargo 命令一律通过 `spike/cargo-serial.ps1` 运行，它会持有互斥锁 `Local\Daozai.Cargo`、并行度为 1。每个工作树只用自己目录下的 `target`，不设 `CARGO_TARGET_DIR`。新工作树第一次编译大约 12 分钟。
- 这四个任务**只提交到自己的分支**。不合进 `test/acceptance` 或 `main`，不编译 release，不替换桌面「岛仔－当前测试版」，不推送到 GitHub。验收、合并和出包由验收方统一做。
- 四个分支最后要合到一起，所以改动尽量放进新文件。共享文件只做增量修改：能力表 `CAPABILITIES`、`spike/local.rs` 的 `Output`、文档里的能力表格、教程能力清单（`spike/plugin_guide_ui.rs` 约 225 行）。
- 插件提供的设置和选项，在所属设置页标题旁显示「插件」小标。插件开关开为紫色、关为灰色，复用宿主现有组件。设置页的说明和关键提醒不要删。状态提示用颜色和动效表达，不加文字标签。
- 文件多为 CRLF 换行，编辑时保持原样。
- 每个任务都要同步更新 [sdk-v1-design.md](../sdk-v1-design.md)、[sdk-v1.schema.json](../sdk-v1.schema.json) 和教程能力清单。

## 交付时回报

- 分支名和提交号。
- 跑了哪些测试，结果如何。
- 没做或做不到的部分，以及原因。
