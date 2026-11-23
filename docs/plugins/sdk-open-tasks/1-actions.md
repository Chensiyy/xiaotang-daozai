# 第 1 步：按钮、表单、事件与提醒

目标是让脚本插件能交互：响应点击，接收专注结束事件，发一次性提醒。分支为 `feat/plugin-actions`，共同规则见 [README](README.md)。

## 先读

- `spike/capabilities.rs`：能力表。
- `spike/local.rs`：重点看 `Manifest`、`Output`、`render_input`、`tick_features`。
- `src/plugin_status.rs`、`src/plugin_status_ui.rs`：状态页的数据和绘制。
- `src/plugin_notice.rs`：现有的通知、未读和点击回执。
- `docs/plugins/sdk-v1-design.md`。

## 要做的

### 按钮与表单（能力 `action`）

- 必须同时声明 `status.page`。
- 状态页输出 `status` 增加两个字段：
  - `actions`：最多 3 个 `{id, label}`，label 最多 6 个字。
  - `fields`：最多 3 个单行文本框 `{key, label, value?, max}`，max 不超过 200。
- `StatusLayout` 新增 `actions` 和 `fields` 两种组件，用宿主现有控件绘制。
- 用户点按钮时，宿主在后台调用脚本的全局函数 `action(input)`：
  - input 为 `{settings, storage, today, source?, action:{id, form:{key:value}}}`。
  - 限制和 refresh 相同：30 ms、4 MiB、输出 16 KiB。
  - 返回值格式和 refresh 相同，结果同样走现有的暂存和替换流程。
- 每个插件同一时间只跑一次调用。
- 插件停用、设置改动或模块关闭后，迟到的结果一律丢弃，复用现有的 cancelled 机制。
- 按钮只对产生它的那次输出有效。输出更新后，旧按钮的点击作废。

### 事件（能力 `event`）

- 清单新增可选字段 `events`，目前只接受 `"focus.finished"`。
- 在 `src/` 里找到专注计时结束的位置，把事件交给插件宿主。
- 宿主调用 `event(input)`，input 带 `event:{name, id, at, minutes}`。
- 同一事件对每个插件只投递一次。
- 插件停用或所属模块关闭期间，事件直接丢弃，不排队。

### 提醒（能力 `notice`）

- 输出可带 `notice:{id, title, body?}`。title 最多 40 字，body 最多 120 字。
- 用岛上现有的提醒来呈现，优先复用 `src/plugin_notice.rs` 的通知和未读机制，不要另造一套。
- 同一 id 只提醒一次，每个插件 60 秒内最多一条。
- 遵守提醒总开关和「显示通知正文」隐私开关。

### 其他

- 这三项能力各在 `CAPABILITIES` 里加一行。需要时扩展 `Role`，组合规则写进 `check()`。
- 在 `spike/examples/` 加一个样例插件，封面可以复制 payday 的。样例要同时用到这三项，比如「喝水记录」：+1 按钮、备注输入框、专注结束自动记一次、满 8 杯提醒。

## 验收标准

- 单元测试至少覆盖：
  - action 往返，以及 storage 写回。
  - 插件停用或输出更新后，旧按钮的点击作废。
  - 同一事件只投递一次。
  - 提醒去重和限频。
  - 清单组合的非法情况。
- egui 界面测试：点状态页按钮后输出更新。写法参考 `spike/local_management_test.rs`。
- 下面这条命令全部通过，和改到的 `src` 模块相关的定向测试也全部通过：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File spike/cargo-serial.ps1 test --locked --lib --features "sdk-js sdk-material" --target x86_64-pc-windows-msvc sdk_spike
```
