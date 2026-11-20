# GPU 材质能力：分阶段任务

设计见 [gpu-material.md](gpu-material.md)。四个阶段按顺序做，每个阶段开一个新对话，上一阶段审查通过后再开下一阶段。开新对话时直接发：

> 按 `D:\A-TangHao\REA\Windows小工具\01-专项工程\GPU材质\docs\plugins\gpu-material-tasks.md` 做第 N 阶段。

## 所有阶段通用

- 工作树：`D:\A-TangHao\REA\Windows小工具\01-专项工程\GPU材质`，分支 `feat/gpu-material`。只在这个工作树里改，不碰 `main` 和其他工作树。
- PR #235 原始代码在分支 `test/pr235-liquid-glass`，用 `git show test/pr235-liquid-glass:<路径>` 阅读，或打开只读参考工作树 `D:\daozai-build\tmp\pr235-preview`。不要在参考工作树里改代码或提交。
- 编译、测试遵守根目录 AGENTS.md：持 `Local\Daozai.Cargo` 锁排队，`CARGO_BUILD_JOBS=1`。release 编译需要 `RUST_MIN_STACK=67108864`。
- 插件相关代码受 `sdk-js` 特性控制，检查和测试都加 `--features sdk-js`。材质相关新代码放在新的 `sdk-material` 特性后面，测试时两个特性一起开。
- 设计稿里的接口（清单字段、`Frame` 布局、prelude 函数）是实验接口，本轮按现有内容实现。需要改动时先在回报里说明原因，不要自行改设计稿。
- 每个阶段结束：跑完相关测试并提交。在最后一条回复里列出提交号、改动文件、测试命令和结果、没做完或有疑问的地方，交给审查。
- 是否出包按各阶段说明，不按 AGENTS.md 的默认出包。

## 第 1 阶段：同步 main，移植透明交换链和抓屏底座

目标：本体具备抓取岛体背后底片和透明交换链的能力，没有材质时完全不运行，外观和行为与 `main` 一致。

1. 把 `main` 合进 `feat/gpu-material`。插件分支和 main 的冲突，参考 `test/acceptance` 上提交 `a3f5c8d` 的解决方式：插件设置页用 `SETTINGS_TAB_PLUGINS = 17`，`render_page` 和 `set_page_visible` 两边都保留。
2. 移植 vendor 补丁，从 `test/pr235-liquid-glass` 取：
   - `vendor/wgpu-hal`（整个目录，与上游 24.0.4 只差 `auxil/dxgi/conv.rs`、`dx12/adapter.rs`、`dx12/mod.rs`）；
   - `vendor/egui-winit`（与上游 0.31.1 只差 `src/lib.rs` 的无重定向位图那几行）；
   - `vendor/egui-wgpu/src/winit.rs`、`src/lib.rs`、`Cargo.toml` 的 DirectComposition 改动，以及 `vendor/eframe/src/native/run.rs`；
   - 根 `Cargo.toml` 里对应的 `[patch]`、路径依赖和 windows 特性。

   main 上可能有 #246 相关的 eframe 改动，合并时保留 main 的版本，再叠加 PR 的改动。
3. 新建 `src/material/backdrop.rs`：移植 PR 的 `src/wgc.rs` 和 `src/gpu_glass.rs` 里的抓屏部分（`capture`、`grab_raw`、干净底片 `CleanPlate`、`compose_with_plate`、背景亮度、捕获排除），整理成 `Backdrop` 对外接口。去掉 `DAOZAI_LIQUID_*` 调试环境变量，诊断日志写到 `diagnostics/material.log`。
4. 透明交换链只在材质生效时启用。PR 里用 `DAOZAI_DCOMP` 环境变量控制，改成由 `material` 模块在启动时根据「兼容渲染已开且有生效的材质」决定。没有材质时不设无重定向位图，走原来的路径。
5. 测试：
   - 现有测试全部通过（`cargo test --lib --features sdk-js`，以及加上 `sdk-material` 再跑一遍）；
   - 给底片几何换算、尺寸变化不复用旧纹理、失败后退回这几处补单元测试。
6. 不出包。

## 第 2 阶段：通用材质管线和控件登记

目标：本体能加载一段符合接口的 WGSL，叠加在任何皮肤上绘制。用内置调试材质验证。

1. 新建 `src/material/prelude.wgsl`：按设计稿定义 `Frame`、绑定和 `param`、`rr_sdf`、`notch_sdf`、`body_sdf`、`sample_backdrop`。函数体从 PR 着色器搬，计算保持一致。
2. 新建 `src/material/pipeline.rs`：
   - 拼接 prelude 和插件代码，在 `push_error_scope` 里编译；
   - 建绑定组、打包 `Frame`（`#[repr(C)]`，每项 16 字节对齐，和 WGSL 布局逐项对应，补一个布局大小的单元测试）；
   - 挂 egui paint callback，预乘 alpha 混合方式与 PR 相同。
3. 新建 `src/material/controls.rs`：
   - 页面绘制时调用 `material::control(id, rect, radius)` 登记控件玻璃；
   - 宿主按清单的刚度和阻尼算弹簧位移、悬停和按下状态。
   
   PR 在 `app.rs` 的 `Ctl` 枚举里列出的 13 个控件（播放、切歌、音量、歌词、齿轮、翻页箭头、进度圆钮、添加账号、邮箱去添加、日程编辑等），逐个在对应页面的绘制代码里登记。
4. `src/material/mod.rs`：
   - 当前材质的加载、启停、每帧入口；
   - 静止 15 Hz、2 分钟后 1 Hz 深休眠；
   - 连续 120 帧材质通道超过 4 ms 自动停用；
   - 任一步失败永久退回普通绘制并记日志。
5. 内置调试材质 `src/material/debug.wgsl`：玻璃内原样输出底片，控件区域描一圈边，用来检查抓屏对齐和控件位置。只能通过测试或 `DAOZAI_MATERIAL_DEBUG=1` 启用，不出现在设置里。
6. 测试：
   - 着色器拼接后能通过 naga 校验（单元测试里用 wgpu 的 naga 解析 prelude 加调试材质）；
   - 写错的着色器被拒绝且不崩溃；
   - 关闭材质时渲染结果与 main 相同（沿用现有 UI 截图测试）。
7. 出包：合进 `test/acceptance`，编测试版替换桌面测试版。回报里写明怎样用 `DAOZAI_MATERIAL_DEBUG=1` 打开调试材质查看对齐效果。

## 第 3 阶段：接入插件 SDK

目标：材质能作为插件安装、启用、调参和停用。

0. 第 2 阶段审查遗留：停靠方向目前用 `@binding(3)` 的 `DaozaiHost` 传入，插件着色器也能读到它，等于多了一个未写进接口的字段。改为放进 `Frame.count.y`（0 顶部、1 左侧、2 右侧），删除 `binding(3)` 和 `DaozaiHost`，`Frame` 仍为 1024 字节。同步更新设计稿 `Frame` 注释，`prelude.wgsl` 里 `body_sdf` 改读 `frame.count.y`，补一条单元测试确认插件声明 `binding(3)` 也会被拒绝。

1. 清单：
   - `spike/local.rs` 的校验接受 `runtime: "material"`、`owner: "appearance"`、`capabilities` 含 `appearance.material`；
   - 新增 `material` 字段（`controls`、`spring`）和 `number` 设置类型（`min`、`max`、`step`，最多 32 个）；
   - 包内只允许 `plugin.json`、一个 `.wgsl` 入口（≤ 64 KB）和说明文件。
   
   同步更新 `docs/plugins/sdk-v1.schema.json` 和 `docs/plugins/sdk-v1-design.md` 的能力表。
2. 启用流程：
   - 首次启用时确认 `screen.backdrop` 权限，说明文字见设计稿；
   - 兼容渲染没开时提示开启并重启，期间插件显示为等待状态，基础皮肤不变；
   - 同一时间只启用一个材质，启用新的会停用旧的。
3. 设置：
   - 「设置 → 外观」里在基础皮肤下面加「材质」一项，列出已安装的材质插件和「无」；
   - 选中后按清单生成参数滑块，改动实时生效；
   - 「我的插件」页同样显示材质插件。
   - 遵守 AGENTS.md 和用户偏好：不删现有设置项和说明，状态用颜色和动效表示，不加文字标签。
4. 截图排除：`material/backdrop.rs` 的 `CaptureExclusion` 目前直接调用 `SetWindowDisplayAffinity`，和 `native.rs` 里 #197 的 `capture::Exclusion` 管的是同一个窗口标记。改为统一经由 `capture::Exclusion`，避免两处互相覆盖。材质生效期间强制排除捕获，「截图和录屏隐藏岛仔」开关显示为已开启并附说明，停用后恢复用户原来的选择。这一条是设计稿「待确认的决策」第 1 条的建议方案，回报时单独列出实现方式供审查。
5. 测试：
   - 清单校验（合法、缺字段、超范围、非 number 设置、包内多余文件）；
   - 启停后设置恢复；
   - 权限未确认时不启用；
   - 兼容渲染关闭时不启用；
   - 截图排除的恢复。
6. 不出包，第 4 阶段一起出。

## 第 4 阶段：液态玻璃插件包和效果对比

目标：液态玻璃作为第一个材质插件，效果与 PR #235 预览版一致。

1. 新建 `spike/examples/liquid-glass/`：
   - `plugin.json`：作者「轻九九九（@cayww）」，参数和默认值取自 PR #235 `model.rs` 的 `liquid_*` 默认值：

     | 参数 | 默认值 |
     |---|---|
     | 折射 | 48 |
     | 色散 | 8 |
     | 内部模糊 | 1.5 |
     | 折射带宽 | 87 |
     | 白边宽度 | 1.5 |
     | 白边不透明度 | 0.9 |
     | 形变 | 1.0 |

     弹簧刚度 1.0、阻尼 0.5。
   - `material.wgsl`：从 PR 的 `gpu_glass.rs` 着色器移植 `fs` 及其辅助函数，改为 `material()` 入口，专用 uniform 改从 `param()` 读取，几何和底片改用 prelude 提供的函数。
2. 效果对比：
   - 同一壁纸、同一页面、同一参数下，分别用参考工作树里的预览版（`D:\daozai-build\tmp\打开液态玻璃预览.cmd`）和测试版截图；
   - 至少覆盖音乐页展开、收起胶囊、控件按下三种状态；
   - 差异明显时先修正移植，并在回报里附对比图路径。
3. 测试：
   - 插件包通过清单校验并能安装、启用、停用；
   - 着色器通过 naga 校验；
   - 关闭插件后外观回到原皮肤。
4. 出包：合进 `test/acceptance`，编测试版替换桌面测试版。在回报里写明安装插件包的步骤，让用户在测试版里试用。插件包只用于本机验收，不发布到插件市场。
5. 接口反馈：回报里单列「接口问题」一节，作为定插件技术规范的依据。写明移植中遇到的不够用或别扭的地方，例如：
   - 控件 16 块、参数 32 个是否够用；
   - `Frame` 缺少或多余的字段；
   - prelude 函数是否需要增减；
   - 弹簧参数放在清单里是否合适；
   - 参数滑块的类型和范围表达是否够用。
   
   每条写现状、问题和建议，不在本阶段自行改接口。
