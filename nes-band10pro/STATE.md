# STATE

- 依据的基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`（https://github.com/oraserz/workspace.git，已 `pull --ff-only`，未推送）
- 上次执行时间：2026-10-27 08:59 CST
- 当前阶段：阶段 3 已完成（阶段 1/2 的交付物已补齐并复核）

## 已完成

- 阶段 1
  - vendor 克隆并记录 commit：Canopus `4928f1eb…`、LyraPlayer `8b228e37…`、x-nes `ca4c3955…`、Mi8WfBinTool `5e4195ce…`。
  - 工具链固定：`rust-toolchain.toml` = 1.97.0 + `thumbv8m.main-none-eabi`。
  - `resource.bin` 解包分析完成（Minecraft 表盘：Lua 安装器 + 726540 B ARM ELF + PNG；preview 336×480）；写入 `docs/01-research.md`。
  - `docs/DECISION.md` 完成（模块形态、上屏路径、帧率、ROM、x-nes 补丁、分配器）。
- 阶段 2
  - `nes_band` 模块实现：x-nes 接入、NROM 解析、RGB565 转换、LVGL 页面 + 100 ms 定时刷新、两阶段原生应用注册。
  - `canopus build` + ELF verifier PASS（0 undefined、18655 relocs、1 ctor / 1 dtor、334232 resident bytes）。
  - `.canopus` 打包成功，`canopus package verify` OK。
  - `docs/02-implementation.md` 完成。
- 阶段 3
  - 主机侧用设备同一份 `video.rs` 导出 3 帧（256×240 PPM、336×480 PPM/PNG），逐像素确认尺寸、黑边、居中与滚动变化；见 `REPORT.md` §3.3。
  - 包结构/元数据/哈希/target 绑定检查完成（manifest、CMI2 收据、sha256、`readelf`）。
  - `REPORT.md` 完成（构建步骤、绝对路径、证据等级、已知问题、设备侧待验证项、基址 commit）。
  - 清理临时文件：删除 `host/target`、`nes_band/build/cargo` 与 `/tmp` 解包产物；保留 `vendor/`、ELF、`.canopus`、`out/` 证据帧。

## 未完成

- 设备侧全部验证：模块加载、两阶段应用注册、LVGL 内存图像源实际绘制/刷新、`bt_alloc` 批准与 244 KiB 分配、单帧耗时。
- 音频、按键、存档（本轮范围外）。
- 帧率标定（当前 100 ms/帧，主机 2.7 ms/帧，M33 未测）。
- mapper 覆盖仅 NROM（mapper 0）经过验证。

## 阻塞原因

- 无构建/打包阻塞；设备验证需要用户在小手环上安装 `dist/nes_band-0.1.0.canopus` 后反馈。

## 下次从这里继续

1. 设备安装 `nes_band/dist/nes_band-0.1.0.canopus`，先看 `canopus_mod_query` 是否返回 MAGIC `NES1`、`ACTIVE=1`、`LAST_ERROR` 是否非 0（`-2` 表示内存分配失败）。
2. 若画面不刷新：检查 `lv_image_set_src` 传内存 `lv_image_dsc_t` 是否被该固件接受；回退方案为写 `/data/canopus/*.bin` 后按路径设置源。
3. 若内存失败：收窄 mapper 支持或降低画面缓冲（如改为调色板索引 + 自绘），或确认 `bt_alloc` 池上限。
4. 用 status 里的帧数/指令数估算单帧耗时，再决定是否把 `FRAME_PERIOD_MS` 调到 50 ms（20 fps）。
5. 之后再做按键与音频。
