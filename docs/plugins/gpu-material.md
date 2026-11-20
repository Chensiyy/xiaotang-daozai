# GPU 材质能力设计稿

状态：设计稿，待实现。分支 `feat/gpu-material`，工作树 `01-专项工程/GPU材质`。分阶段任务见 [gpu-material-tasks.md](gpu-material-tasks.md)。

## 目标

让插件给岛体换上实时 GPU 材质：岛体背后的桌面透过材质折射、模糊或变色，控件跟着有独立的玻璃块和弹性。第一个材质插件是液态玻璃，效果以 PR #235（作者 轻九九九，@cayww）的预览版为准，做成插件后视觉保持一致。

本体只提供通用能力，不含任何具体材质的算法和参数。以后其他作者做磨砂、金属、霓虹等材质，只交一段 WGSL 着色器和一份参数清单。

## 分工

| 本体（宿主）提供 | 材质插件提供 |
|---|---|
| 岛体背后的桌面底片和平均亮度 | WGSL 着色器（`material.wgsl`） |
| DirectComposition 透明交换链，真实逐像素 alpha | 参数清单：名称、范围、默认值 |
| 把着色器接进 egui 的 wgpu 渲染通道，按固定格式传入岛体轮廓、控件位置、指针、时间 | 控件弹簧的刚度和阻尼 |
| 控件玻璃区域登记和弹簧动画 | 作者、版本、说明 |
| 着色器检查、出错和超预算时停用并退回普通绘制 | |
| 根据参数清单生成设置滑块 | |
| 静止降频、深休眠、抓屏节流 | |

## 运行条件

- 只在「兼容渲染（Direct3D 12）」开启时生效。默认的 glow（OpenGL）路径没有 wgpu 设备。启用材质插件时，兼容渲染未开则提示开启并重启，基础皮肤保持不变。
- 同一时间只有一个材质生效。材质叠加在用户当前的基础皮肤和形状上，停用插件后回到原样。皮肤、形状、停靠仍由用户掌管。
- 设置窗口和其他窗口不受影响，只作用于主岛。
- 任何一步失败（建不起交换链、抓不到屏、着色器编译失败、连续超预算）都永久退回普通绘制，直到用户重新启用，不影响岛仔其他功能。

## 插件清单

在现有 SDK v1 清单上新增 `runtime: "material"`：

```json
{
  "id": "cayww.liquid-glass",
  "name": "液态玻璃",
  "author": "轻九九九",
  "version": "1.0.0",
  "api": 1,
  "runtime": "material",
  "entry": "material.wgsl",
  "owner": "appearance",
  "capabilities": ["appearance.material", "settings"],
  "dataVersion": 1,
  "permissions": {"screen.backdrop": true},
  "material": {
    "controls": true,
    "spring": {"stiffness": 1.0, "damping": 0.5}
  },
  "settings": [
    {"key": "refract", "label": "折射强度", "type": "number", "min": 0, "max": 120, "step": 1, "default": "48"},
    {"key": "chroma", "label": "色散", "type": "number", "min": 0, "max": 24, "step": 0.5, "default": "8"}
  ]
}
```

- `entry` 必须是包内的 `.wgsl` 文件，大小上限 64 KB。包内不允许脚本和可执行文件。
- `settings` 新增 `number` 类型，带 `min`、`max`、`step`。最多 32 个数值参数，按声明顺序依次放进着色器的 `params` 数组（见下文）。材质插件只接受 `number` 类型的设置。
- `permissions.screen.backdrop`：读取岛体背后的屏幕画面。首次启用时确认，说明文字：「读取岛体背后一小块屏幕画面，只在本机用于绘制材质，不保存、不上传；插件本身没有网络和文件权限。」实现上，宿主把裁剪后的像素读回岛仔进程内存，用于计算亮度和合成干净底片，再上传为显卡纹理；像素不写入磁盘和日志，插件只能在着色器里采样纹理。
- `material.spring`：控件弹簧的刚度和阻尼，由宿主计算弹簧位移后传给着色器。

## 着色器接口

宿主在插件着色器前面拼接一段固定前置代码（prelude），里面定义输入结构、绑定和常用函数。插件只写片元函数 `fn material(i: MaterialIn) -> vec4<f32>`，返回预乘 alpha 的颜色。顶点着色器由宿主提供（全屏三角形覆盖抓取矩形）。

```wgsl
struct Frame {
  capture : vec4<f32>,   // 抓取矩形在屏幕上的物理坐标 x,y,w,h
  body    : vec4<f32>,   // 岛体在抓取矩形内的 x,y,w,h
  shape   : vec4<f32>,   // 圆角半径 px、展开程度 0..1、指针 x、指针 y（抓取矩形内 px）
  notch   : vec4<f32>,   // 刘海外扩宽度 px、外扩高度 px、是否刘海、保留
  env     : vec4<f32>,   // 时间 s、背景平均亮度 0..1、物理像素/逻辑点、是否透明交换链
  tint    : vec4<f32>,   // 封面主色 rgb、强度
  tex     : vec4<f32>,   // 底片纹理宽、高、有效区宽、高
  count   : vec4<u32>,   // x 控件数量，y 停靠方向（0 顶部、1 左侧、2 右侧），z/w 保留
  ctl_rect  : array<vec4<f32>, 16>, // 控件 x,y,w,h（抓取矩形内 px）
  ctl_shape : array<vec4<f32>, 16>, // 圆角半径、启用、悬停 0..1、按下 0..1
  ctl_spring: array<vec4<f32>, 16>, // 弹簧位移 x、y、缩放、保留
  params  : array<vec4<f32>, 8>,    // 插件参数，按清单 settings 顺序，每 4 个一组
};
@group(0) @binding(0) var<uniform> frame : Frame;
@group(0) @binding(1) var backdrop : texture_2d<f32>;
@group(0) @binding(2) var backdrop_sampler : sampler;

struct MaterialIn { px : vec2<f32>, uv : vec2<f32> };
fn param(i : u32) -> f32 { return frame.params[i / 4u][i % 4u]; }
fn rr_sdf(p : vec2<f32>, b : vec2<f32>, r : f32) -> f32 { ... }      // 圆角矩形
fn notch_sdf(p : vec2<f32>, b : vec2<f32>, r : f32, flare : f32, fh : f32) -> f32 { ... }
fn body_sdf(p : vec2<f32>) -> f32 { ... }                             // 当前岛体轮廓，已处理刘海和展开
fn sample_backdrop(px : vec2<f32>) -> vec3<f32> { ... }               // 按抓取矩形内像素坐标采样底片
```

- 布局一经发布即为 `api: 1` 的稳定接口，只能在末尾追加字段。PR #235 里液态玻璃专用的 uniform（ior、dispersion、look、morph 等）全部改为插件参数，从 `params` 读取。
- 控件上限 16 块，超过时按面积从大到小保留。
- `rr_sdf`、`notch_sdf` 和 `sample_backdrop` 取自 PR #235 的实现，保证液态玻璃搬过来后计算一致。

## 宿主模块

新代码放在 `src/material/`，主岛渲染只通过一个入口调用：

| 文件 | 内容 | 来源 |
|---|---|---|
| `material/mod.rs` | 对外入口：当前材质、启停、每帧挂回调、失败退回、预算 | 新写 |
| `material/backdrop.rs` | 抓取外扩矩形、干净底片、背景亮度、显示器拓扑变化后的稳定期 | PR 的 `wgc.rs` 和 `gpu_glass.rs` 抓屏部分 |
| `material/pipeline.rs` | 拼接 prelude、编译检查、绑定、uniform 打包、egui paint callback | PR 的 `gpu_glass.rs` 管线部分，去掉液态玻璃专用字段 |
| `material/controls.rs` | 控件玻璃区域登记、弹簧动画 | PR 的 `app.rs` 中 `Ctl` 和 `glass_controls`，改成页面绘制时登记 |
| `material/prelude.wgsl` | 前置代码 | PR 着色器里的公共函数 |
| `vendor/egui-wgpu/src/winit.rs` | DirectComposition 透明交换链 | PR 原样移植 |
| `vendor/wgpu-hal`、`vendor/egui-winit` | 预乘 alpha 交换链、取 D3D12/DXGI 设备、无重定向位图窗口，共约 50 行补丁 | PR 原样移植，其余文件与上游 24.0.4 / 0.31.1 相同 |

控件登记：各页面在绘制按钮、滑块、卡片时调用 `material::control(id, rect, radius)`，不再由 `app.rs` 维护一张写死的控件表。没有材质时这个调用什么也不做。

## 性能与稳定

- 静止时按 15 Hz 刷新，无操作 2 分钟进入 1 Hz 深休眠，动画和交互期间满帧。数值沿用 PR #235 的默认值，作为宿主设置，不交给插件。
- 抓屏只读外扩矩形，大小变化时不复用旧纹理（PR 注释里的错位问题）。
- 着色器编译放在 `push_error_scope` 里，失败即停用并提示「材质加载失败」。
- 每帧记录材质通道的 CPU 耗时，连续 120 帧超过 4 ms 自动停用。GPU 时间戳查询作为后续优化。
- 启用、停用、切换材质时清理纹理和管线，内存回到启用前水平（验收时对比工作集）。

## 待确认的决策

1. **截图排除**：WGC 要拿到干净的桌面，岛体窗口需要排除出捕获（`WDA_EXCLUDEFROMCAPTURE`）。这会让截图和录屏里看不到岛仔，和设置里的「截图和录屏隐藏岛仔」（#197）是同一个开关。建议：材质生效期间强制排除，设置里那个开关显示为已开启并说明原因，停用材质后恢复用户原来的选择。
2. **独立显卡优先**：PR 有「优先独立显卡」选项。它影响整个岛仔，建议作为兼容渲染的通用设置单独评估，不放进本轮。
3. **默认渲染**：本轮不改默认渲染。是否把 wgpu 改成默认，另行评估。

## 不在本轮

- PR #235 中对所有皮肤生效的性能改动（停用分层读回、工作集回收等），由 #246 的修复单独评估。
- 「液态」专用的 `Skin::Liquid` 皮肤枚举和设置项，不进入本体。
- 插件市场分发和签名。

## 验收

- 关闭材质插件时，岛仔各皮肤的显示、内存、帧率与 `main` 一致，现有测试全部通过。
- 启用液态玻璃插件后，与 PR #235 预览版在同一壁纸、同一页面、同一参数下并排截图对比，折射、色散、白边、控件玻璃和果冻弹簧一致。
- 关闭兼容渲染、着色器写错、抓屏失败三种情况下都退回普通绘制，岛仔不崩溃。
- 停用插件后外观回到启用前的皮肤和形状，截图排除恢复用户原设置。
