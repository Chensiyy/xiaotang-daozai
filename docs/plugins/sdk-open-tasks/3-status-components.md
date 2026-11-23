# 第 3 步：状态页组件与主题令牌

扩充插件状态页的组件，并加入受控的主题令牌。分支为 `feat/plugin-status-components`，共同规则见 [README](README.md)。

## 先读

- `src/plugin_status.rs`：`PluginStatus`、`StatusLayout`、`StatusComponent`、`StatusStyle`。
  - 已有组件：state、summary、progress（环或条）、metrics、alert、updated_at。
  - 已有样式：accent、density、surface。
- `src/plugin_status_ui.rs`：状态页的绘制。
- `spike/status_source_tests.rs`。
- `docs/plugins/sdk-v1-design.md` 里的状态页说明。

## 要做的

### 新组件

每种组件做三件事：在 `PluginStatus` 里加一个数据字段，在 `StatusComponent` 里加一个类型，在 `valid()` 里设上限。

- `list`：`items` 最多 8 项 `{label, detail?, state?}`。`state` 复用 `StatusState`，用颜色点表示。
- `chart`：`series` 是 2 到 48 个有限数字，画成迷你折线。宿主负责面积填充、淡网格和末点强调。
- `text`：`note`，最多 300 字的多行说明。
- `badge`：`tags` 最多 4 个短标签，每个最多 8 字。

布局上限从 6 个组件放宽到 8 个，每种组件仍只能出现一次。

### 主题令牌

`StatusStyle` 新增两个令牌：

- `emphasis`：取 `normal` 或 `strong`。
- `icon`：从宿主内置图标里挑一小组名字开放，例如 printer、battery、cloud、cup、clock、chart。没有对应图标的名字就不开放。

accent 在绘制前按当前皮肤的卡片底色自动校正，对比度至少 3:1。字体、圆角、间距和动效仍由宿主决定，不开放给插件。

### 不要做的

按钮和输入框不做，它们属于第 1 步，那边会在同一个枚举里加 `actions` 和 `fields`。这边只要保持枚举容易合并就行。

### 紧凑密度

紧凑密度下，由宿主决定收起哪些组件，或者只显示前几项。

### 样例与文档

- 在 `spike/examples/` 加一个模拟打印机状态页的样例。封面可以复制 payday 的。尽量用上大部分组件和令牌，数据由脚本模拟，不联网。
- 更新 `docs/plugins/sdk-v1.schema.json` 的 `statusLayout` 部分。

## 验收标准

- 单元测试覆盖三类情况：新字段的上限和非法值、布局上限、accent 对比度校正。
- egui 绘制测试：同一份状态在以下条件下渲染，文字都不越出卡片，也不 panic。
  - 不同皮肤，以及深色和浅色主题；
  - 标准和紧凑两种密度；
  - 100% 和 200% 缩放。
- 旧的 statusLayout 插件包不用改，照样能通过。
- 下面这条命令全部通过，然后把最后的过滤词换成 `sdk_spike` 再跑一次，也要全部通过：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File spike/cargo-serial.ps1 test --locked --lib --features "sdk-js sdk-material" --target x86_64-pc-windows-msvc plugin_status
```
