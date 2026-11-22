# AI 额度来源接口

在 Windows 版岛仔的「设置 → AI 额度 → 本地额度插件」中，选择插件目录内的 `plugin.json`。安装后默认停用，可在同一处启用、管理数据源授权或卸载。插件有读数时，AI 额度面板以清单中的名称显示它的来源页签。

## 插件清单

| 字段 | 取值 |
| --- | --- |
| `api` / `dataVersion` | `1` / `1` |
| `runtime` / `owner` | `"js"` / `"quota"` |
| `id` / `name` / `author` / `version` | 唯一的点分插件 ID、显示名称、作者和三段版本号 |
| `entry` / `cover` | 插件目录内的 JavaScript 入口、根目录的 `cover.png` |
| `capabilities` | 必需 `quota.source`。定时刷新加 `timer`，读写插件自己的存储加 `storage` |
| `permissions` | `{}`，或声明 `source.read` 为 `"file"` 或 `"loopback"` |
| `refreshSeconds` | 有 `timer` 时的刷新间隔，60 至 86400 秒，默认 60 秒 |
| `settings` | 可填 `[]`。有自定义字段时声明 `settings` 能力，宿主提供文本或日期字段的默认值 |

## 读取与返回值

入口定义 `refresh(input)`。有读取权限声明时，`input.source.status` 为 `unconfigured`、`ready` 或 `offline`。仅 `ready` 有 `input.source.text`。文件来源由用户选定一个本地 UTF-8 文件。本机接口固定到用户批准的完整回环 HTTP 地址，只发送 GET 请求。`input.settings`、`input.storage` 和 `input.today` 分别提供本插件的设置、私有存储和本地日期。

返回对象的 `quota` 字段含 `windows` 数组，每次提供 1 至 3 项。每项填写 `label`，并选择 `remaining_pct`（0 至 100）或 `remaining` 和 `unit`（可加正数 `limit`）。每项可加 `period_secs` 和 `reset_at`，分别表示周期秒数和重置 Unix 时间戳。`quota.expires_at` 可提供到期 Unix 时间戳。

返回 `quota: null` 会撤下读数。停用、撤权或关闭额度模块时立即撤下对应来源，旧读取结果作废。断源时，宿主在刷新发现读取失败后撤下该来源。额度读数只保留在本次运行中，宿主独立维护内置账号和其他插件的额度。
