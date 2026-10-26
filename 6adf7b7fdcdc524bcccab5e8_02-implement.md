# 阶段 2：实现与上屏

规范见 `TASKSPEC.md`，决策见 `docs/DECISION.md`。目标：一个能编译、能打包、能把 NES 帧画到屏幕的模块。

## 本次要做

1. 接续 `STATE.md`，不要重复已完成的工作。
2. 建正式模块工程（portable 或 target-bound，按 `DECISION.md` 的选择）。
3. 集成 x-nes：优先 Rust crate；ABI/工具链不合适则 staticlib + C FFI。明确内存来源（模块动态堆约 128K）。
4. ROM 读取：优先 `/data/canopus` 或模块 data 路径下的 `.nes`；脚本生成最小 NROM 测试 ROM（纯色/棋盘格/滚动色块），不得提交版权 ROM。
5. 帧路径：模拟 → 256×240 调色板索引 → 按 `ctx->display->get_resolution` 做整数倍缩放/黑边 → 转成目标像素格式 → 刷新上屏。帧率稳定优先，允许跳帧/抽样，目标 20–30fps，记录每帧耗时。
6. 若 UI ABI 的 IMAGE/VISUAL 无法逐帧刷新，按 `DECISION.md` 的回退方案走 target-bound LVGL（lv_canvas / lv_image_dsc），并在文档标注证据等级。
7. `canopus build` + ELF verifier 通过；`canopus package` 产出 `.canopus` 包。
8. 写 `docs/02-implementation.md`，更新 `STATE.md`。

## 完成判据

- 模块可 `canopus build`，verifier 通过；
- `.canopus` 包存在；
- 存在一条明确的、可解释的"帧 → 屏幕"路径；
- 测试 ROM 可被模块加载并跑帧（至少主机侧验证）。

若阶段 2 已完成，按 `prompts/03-verify.md` 推进。
