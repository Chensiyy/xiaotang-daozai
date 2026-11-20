# 本地试装倒计时插件

在 Windows 的 `plugin-sdk-local-install` 分支，用启用了 `sdk-js` 的岛仔打开「设置 → 我的插件」。已在使用岛仔时，先从托盘退出当前实例，再启动试用版。可以双击[试用本地插件.cmd](../../spike/试用本地插件.cmd)编译并启动。等价的 PowerShell 命令在分支根目录运行，试用数据放进本工作树的 `target/local-trial`。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File spike/cargo-serial.ps1 build --locked --features sdk-js --bin daozai --target x86_64-pc-windows-msvc
$env:DAOZAI_DATA_DIR = Join-Path (Get-Location) 'target/local-trial'
& './target/x86_64-pc-windows-msvc/debug/daozai.exe'
```

1. 在「我的插件」展开「开发者模式」，点击「安装本地插件」，选择[发薪倒计时清单](../../spike/examples/payday/plugin.json)。安装后点击「启用」。用相同步骤装上[旅行倒计时](../../spike/examples/trip/plugin.json)。
2. 在发薪插件旁点「前往设置」，在「待办与置物」修改标题和目标日期并保存。展开岛上的待办页，在原有待办列表下方看卡片。有两张卡时，点卡片可切换。
3. 停用旅行插件，只留下发薪卡。重启同一试用版，旅行仍为停用。关闭「页面管理 → 待办」停止所属插件的刷新，重开待办后仅发薪卡恢复。
4. 卸载发薪插件并选「保留数据」。从原目录重装并启用，核对上次保存的日期。再卸载并选「删除数据」，重装后应显示样例默认日期。旅行插件独立保存自己的设置和状态。

## 作者如何接入

复制一个样例目录，在 `plugin.json` 改插件 ID、名称和设置默认值。宿主按清单的 `entry` 加载 JS。文本与日期字段由清单声明，插件在 `refresh({ settings, today, storage })` 中得到已保存的设置、本机日期和上次提交的私有 JSON 数据。返回的 `card` 是显示文字，`storage` 是本次要保存的快照。无外部权限。此试用实现支持归属待办的本地卡片，刷新由宿主调度。

安装将清单与入口脚本复制到本地记录，后续独立于源码目录运行。要试修改后的包，可选择保留数据卸载，从原清单路径重装。同一 ID 已安装时会提示重复。保留数据用 ID 和本地清单路径识别，仅适用于本人挑选的开发包。默认数据在岛仔配置目录的 `local-plugins` 子目录。

每个包用同一份清单声明 `timer` 和 `refreshSeconds`（60 到 86400 秒，省略时为 60）。每次刷新都从头执行 main.js；下次还要用的值请放进返回的 storage。停用、关闭待办或改完设置后，上一次计算即使返回也不会再显示或保存。action（按钮）和 event（事件）仍待宿主实现。

重启时岛仔重新核对已安装记录的 ID、清单、脚本入口和设置。看到“需要重新安装”时，在开发者模式重新选择可信的原包。未通过检查的记录不会启动；重装会使用原包的默认设置。
