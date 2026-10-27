# STATE

- 依据的基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`（vendor：Canopus `4928f1eb70594263292c32ffc531a7cad3aea555`，x-nes `ca4c3955b8a442fe25ee62abd57fca6849d1e70b`）
- 上次执行时间：2026-10-27 12:50 CST
- 当前阶段：兜底与最终交付（`04-catchup`）已完成，进入设备验证等待
- 已完成：
  - `nespro` 模块接入 x-nes，`canopus build` 通过 ELF verifier（15 sections / 515,869 resident / 0 undefined / 1 ctor）；`canopus clean` 后重建 sha256 完全一致（可复现）
  - `.canopus` 包已产出并通过 `canopus package verify`（1 target，685,056 B，重建后 sha256 一致）
  - 主机侧画面验证完成：4 帧 / 37,632 指令，`frame-04` 13 色，`content-424` 336×424 布局预览
  - 自产 16 KiB NROM 测试 ROM（`roms/nespro-test.nes`）
  - 文档：`docs/01-research.md`、`docs/DECISION.md`、`docs/02-implementation.md`、`REPORT.md`
  - 运行时 `display->get_resolution` 校验；内存峰值优化（`drop(rom)`）；x-nes vendor 编译补丁
- 未完成：
  - 真机安装、页面注册、LVGL 上屏、帧率与栈/堆实测（设备未验证）
  - 音频、按键、存档（明确超出本轮）
  - 大于约 24 KiB 的 ROM、真双缓冲/局部刷新
- 阻塞原因：设备（小米手环 10 Pro，固件 043）不在执行环境内，无法做设备验证；本轮不要求 qemu/Mirage
- 下次从这里继续：
  1. 阅读 `REPORT.md` §7，按设备验证清单在真机安装 `dist/nespro-0.1.0.canopus`，确认日志 `display 336x480`、画面出现且持续刷新
  2. 若栈溢出 → 改为静态原地初始化 `Bus`；若 OOM → 缩小 ROM 或改按 bank 读取
  3. 设备侧计时每帧耗时，决定是否下调 `INSTRUCTION_BUDGET` / 定时器周期
