# 任务规范：小米手环 10 Pro（043）NES 模拟器（仅画面输出）

本文件是多个定时执行会话共用的权威规范。每次执行先读本文件，再读 `STATE.md`。

## 目标

为小米手环 10 Pro（固件 043，Canopus target id `xiaomi-band-10-pro-3.101.043`）编写运行在 Canopus 模块框架内的 NES 模拟器，模拟核心使用 `x-nes`。
本轮只要求把 NES 画面正确输出到手环屏幕；音频、按键、存档后置。
不要求用 qemu/Mirage 验收（设备验收由用户本人完成），但必须交付可复现构建产物与主机侧画面验证证据。

## 日期守卫（每次执行必做）

1. 先确认当天日期。**不是 2026-10-27 就不做任何工作、不修改任何文件**，只回复“该任务仅计划在 2026-10-27 执行，现已过期，请删除此定时任务”并结束。
2. 绝对不要在任何一次执行中创建新的定时任务。
3. 若 `/workspace/nes-band10pro/.run.lock` 存在且未超过 6 小时，说明可能有上一次执行在进行：本次跳过并说明。

## 目录与状态

- 工作目录：`/workspace/nes-band10pro/`
- 状态文件：`STATE.md`，记录：当前阶段、已完成项、未完成项、阻塞原因、下次从哪继续、上次执行时间。
- 允许临时目录，但交付物必须留在工作目录内。

## 必读资料

1. 框架与规范（一切基础）：https://github.com/AstralSightStudios/Canopus
   - `docs/canopus-services-specification.md`（ctx->ui / display / fs / time / mem 等服务）
   - `docs/target-authoring.md`、`docs/cli.md`、`AGENTS.md`、`sdk/rust/README.md`
   - `targets/xiaomi-band-10-pro-3.101.043/target.toml`（armv8-m.main / cortex-m33 / thumb2 / soft float）
   - 模块工程：`canopus new` / `canopus build` / `canopus package`，先跑 `canopus doctor`
2. 参考模块：https://github.com/AstralSightStudios/Canopus-App-LyraPlayer
   - 只借鉴构建、打包、安装表盘、UI 布局与资源组织；逻辑层与具体实现自己写，不要照抄。
3. NES 核心：https://github.com/ChaoswareHQ/x-nes
   - Rust no_std 库；`ffi` feature 导出 `nes_*` C API（`nes_init` / `nes_load_rom` / `nes_reset` / `nes_run_frame` / `nes_get_frame_ptr`；frame 为 256×240 调色板索引）。
   - 优先用 Rust crate 直接集成；若模块 ABI/工具链不适合，用 staticlib + C FFI，并在文档说明取舍。
   - 注意 Rust 版本要求（1.97+）与目标三元组（cortex-m33 / soft float）。
4. 表盘资源：https://github.com/oraserz/workspace 根目录 `resource.bin`（约 784KB，MC 游戏安装表盘）
   - 用 https://github.com/zhy8388608/Mi8WfBinTool 解包：`javac Mi8WfBinTool.java`，再 `java Mi8WfBinTool unpack resource.bin ./out mi8pro`。
   - 10 Pro 归 `mi8pro` 规范；若参数不适用自行判断并记录。
   - 分析图元格式、索引色/RLE 编码、图片尺寸、缩放与控件布局，提炼 NES 帧缓冲上屏的编码/刷新思路；不得直接复用其美术资源发布。
5. 屏幕参数不要凭记忆写死：运行时用 `ctx->display->get_resolution` 并结合解包资源实际尺寸确认，再做整数倍缩放/黑边适配。

## 铁律

- 严禁跨 target 推断固件地址；不得擅自提升 `targets/*/symbols` 的证据等级。
- 不得把“编译通过 / 主机测试 / 模拟器通过”说成“设备验证”。
- 仓库不得提交任何版权 ROM；不得提交固件 bin。
- 单次执行时间有限：结束前必须把状态写回 `STATE.md`，不要无限调试。

## 需要自主决策并写入 `docs/DECISION.md` 的问题

- 模块形态：portable（canopus-runtime / canopus-ui-core）还是 target-bound（canopus-target-private）；优先风险最低、能编译且 ELF verifier 通过的方案。
- 上屏路径：Canopus UI ABI（IMAGE/VISUAL/CAPTURE，单树最多 32 节点）能否支撑逐帧刷新；若不能，评估 target-bound 直接使用 boot-resident LVGL（lv_canvas / lv_image_dsc）或 target 已恢复显示接口，说明证据等级与回退方案。
- 帧率：稳定优先，允许跳帧/抽样，限制 20–30fps；给出每帧耗时与内存核算（模块动态堆约 128K，核算 x-nes 的 RAM/ROM 占用，必要时收窄 mapper 支持范围）。
- ROM：用脚本生成最小 NROM 测试 ROM（纯色/棋盘格/滚动色块）；若用第三方测试 ROM（nestest / 240p Test Suite），先确认许可证并记录来源。

## 阶段与交付

### 阶段 1：调研与基座

- 克隆上述仓库到 `/workspace/nes-band10pro/vendor/`。
- 跑通 `canopus doctor`、`canopus new`、`canopus build`、`canopus package` 的最小工程。
- 交叉编译 `x-nes` 到本 target，做最小链接验证。
- 解包 `resource.bin`，输出格式分析。
- 产出 `docs/01-research.md`、`docs/DECISION.md`，创建/更新 `STATE.md`。

### 阶段 2：实现与上屏

- 实现模块工程，接入 x-nes。
- ROM 读取：优先 `/data/canopus` 或模块 data 路径下的 `.nes`。
- 逐帧模拟、帧缓冲缩放/格式转换、上屏刷新。
- 生成最小测试 ROM。
- `canopus build` 与 ELF verifier 通过。
- 产出可安装 `.canopus` 包与 `docs/02-implementation.md`，更新 `STATE.md`。

### 阶段 3：画面验证与收尾

- 主机侧按与设备相同的转换路径导出帧为 PNG/PPM，确认画面正确。
- 检查包结构、签名与元数据。
- 整理 `REPORT.md`：构建步骤、产物绝对路径、证据等级、已知问题、设备侧待验证项。
- 清理临时文件，更新 `STATE.md`。

## 本任务内的验收标准

1. 工程可一键复现构建。
2. `canopus package` 产物存在并通过 verifier。
3. 主机侧能看到测试 ROM 的正确 NES 画面。
4. 文档明确区分“编译通过 / 主机验证 / 设备未验证”，不得声称设备已可运行。

## 输出

交付物集中在 `/workspace/nes-band10pro/`，最终在 `REPORT.md` 给出产物绝对路径清单。
