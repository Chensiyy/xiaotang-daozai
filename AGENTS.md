# 分支与环境

| 环境 | 对应 | 内容 |
|---|---|---|
| 线上 | `main` 上的版本标签（如 `v0.6.66`） | 用户正在用的正式版 |
| 预发布 | `main` | 已验收、准备进下一版的改动 |
| 测试 | `test/acceptance` | `main` 加上所有待验收的功能分支 |

- 确定无争议的小修复直接提交到 `main`。
- 新功能和较大改动在 `feat/…`、`fix/…` 分支开发，做完先合进 `test/acceptance`，不进 `main`。
- 功能验收通过后才合进 `main`；被否掉的从 `test/acceptance` 撤出，`main` 保持干净。
- `main` 有新提交后，把 `main` 合进 `test/acceptance`。合并冲突在 `test/acceptance` 上解决，不回写 `main`。
- 发版只在 `main` 上进行：改版本号、写 CHANGELOG 和公告、打标签。线上急修而 `main` 有不想带出去的改动时，从线上标签拉 `hotfix/…`，发版后合回 `main`。
- 待验收的功能分支列在 `test/acceptance` 的合并记录里；临时测试分支用完就删，不长期保留。

# 工作树

一条分支对应一个固定工作树：

| 分支 | 工作树 |
|---|---|
| `main` | `D:\A-TangHao\REA\Windows小工具\00-主工程\daozai` |
| `test/acceptance` | `D:\daozai-build\worktrees\acceptance` |
| `feat/…`、`fix/…` | `D:\A-TangHao\REA\Windows小工具\01-专项工程\<功能中文名>` |
| 临时复测、审 PR | `D:\daozai-build\tmp\<事由>` |

- 功能分支用 `feat/` 或 `fix/` 开头，名字说清功能，不带日期。
- 功能合进 `main` 后，删除分支和它的工作树。
- 复测旧版本、审 PR 等临时任务在 `tmp` 下建工作树，任务结束时删除；需要提交的临时分支用 `test/…`，一并删除。
- 工作树不建在 `target`、`D:\Temp` 或其他工作树里面。

# 本机测试版交付

桌面「岛仔－当前测试版」只运行 `test/acceptance` 的编译结果，快捷方式指向测试分支工作树 `D:\daozai-build\worktrees\acceptance` 里的 `target/x86_64-pc-windows-msvc/release/daozai.exe`。任何会话都不用自己的分支或 `main` 直接覆盖测试版。

交付步骤：把改动合进 `test/acceptance`，在该工作树跑测试，再以 `DAOZAI_TEST_BUILD=1` 编译 release。交付前核对快捷方式目标和文件哈希；如果岛仔正在运行，切换到新程序并核对进程路径。后续交付沿用同一入口，不另建临时交付目录。
