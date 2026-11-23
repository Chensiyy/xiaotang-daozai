# 第 2 步：联网取数与凭据

开放受控联网，密钥由岛仔托管，另外给集市加版本撤回标记。分支 `feat/plugin-https-source`，共同规则见 [README](README.md)。

## 先读

- `spike/source_read.rs`：现有的文件和本机 HTTP 只读来源。
- `spike/local.rs`：`source_kind`、`set_source_grant`，以及 `tick_features` 里的后台读取。
- `spike/local_ui.rs`：`source_dialog` 授权弹窗。
- `src/mail.rs` 的 `mod secrets`：已经在用 `keyring` 读写 Windows 凭据管理器。
- `spike/market.rs`、`spike/market_state.rs`。
- `docs/plugins/sdk-v1-design.md` 第 5、12 节。

## 要做的

### 新来源 `https`

清单写法：

- 权限写成 `"permissions":{"source.read":"https"}`。
- 新增可选字段 `network:{hosts:[...], url?, credential?:{label, header, prefix?}}`，只有 https 来源能带。
- `hosts` 列 1 到 4 个完整域名，不接受通配符、IP、端口和 localhost。

授权：

- 新增 `SourceKind::Https` 和 `SourceGrant::Https{url}`。
- 授权弹窗里的地址预填清单的 `url`。用户确认的地址要满足：
  - 是 https；
  - 主机在 `hosts` 里；
  - 没有账号密码段；
  - 端口是 443 或不写。
- 清单声明了 `credential` 时，弹窗多一个遮挡显示的密钥输入框，标题用 `credential.label`。

密钥：

- 存进 Windows 凭据管理器。复用 `keyring`，服务名单独起一个，键用插件 ID。
- 记录 JSON 和日志里绝不能出现密钥。
- 撤销授权或卸载插件时，删除这把密钥。
- 插件更新后 `hosts` 不再包含已授权地址的，视为未授权。

### 读取

- 只发 GET 请求，关闭环境代理。
- 设 `redirects(0)`，3xx 一律算失败。
- 用自定义 resolver，解析后丢弃这几类地址：回环、私网、链路本地、CGNAT、ULA、组播、未指定。丢完为空就算失败。
- 超时 10 秒，响应最多 256 KiB，必须是 UTF-8。
- 密钥由宿主按 `header` 加 `prefix` 注入请求，脚本拿不到。
- 脚本只拿到响应正文，沿用现有的 `source` 输入（ready、offline 等）。
- 每次取数都重新检查一遍上面的地址规则。

### 能力规则

在 `check()` 或 `Manifest::validate` 里写清 https 来源和 `network` 字段的组合规则。

### 版本撤回

- 集市目录的条目可以带 `revoked:["1.0.1", ...]`。
- 已安装的版本被撤回时：停用这个插件，在「已安装」里用警示色标出；有更高的未撤回版本时，提供更新。
- 被撤回的版本不能安装。
- 补上 `spike/market.rs` 的解析，以及对应的测试。

### 样例

- 在 `spike/examples/` 加一个 https 来源样例，读取一个公开、无需登录的 JSON 接口就行。
- 再写一份带 `credential` 的清单示例，放进文档。

## 验收标准

单元测试至少覆盖下面几项，且不依赖外网，用本地假服务或直接测函数：

- 地址校验：非 https、IP、`hosts` 之外的主机、带账号密码、非 443 端口。
- resolver 过滤私网和回环地址。
- 拒绝跳转。
- 超大响应。
- 密钥不出现在记录 JSON 里。
- 撤销授权后停止读取。
- 撤回版本被停用，并且不能安装。

下面这条命令全部通过：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File spike/cargo-serial.ps1 test --locked --lib --features "sdk-js sdk-material" --target x86_64-pc-windows-msvc sdk_spike
```
