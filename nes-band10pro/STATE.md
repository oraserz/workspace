# STATE

- 依据的基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`（`https://github.com/oraserz/workspace.git`，已 `pull --ff-only`，未推送）
- 上次执行时间：2026-10-27 05:12 CST（2026-10-26T21:12Z）
- 当前阶段：**阶段 1、2 已完成（构建 + 打包 + 主机侧验证）→ 阶段 3 主机侧/交付收尾已完成；设备侧验证未开始**

## 已完成

### 阶段 1（调研）
- 工具链、最小工程闭环、x-nes 交叉编译与 ET_REL 链接、`resource.bin` 解包与格式分析。
- 产出 `docs/01-research.md`、`docs/DECISION.md`。

### 阶段 2（实现与上屏）
正式模块 `nesband10pro/`（target-bound Rust、`staticlib`、no_std）：

- `lib.rs`：`prepare` 只绑 context（7.9 KiB 栈、零分配）；`activate` 做全部初始化；`GlobalAlloc` 走 `ctx->mem`；`query` 暴露 9 个 u32（含 `heap_used/available`）；两阶段 `publish_native_app_stage`；**新增 `display_resolution()`** 转调 `ctx->display->get_resolution`。
- `nes.rs`：ROM 查找（模块 data 路径 → `/data/canopus/nesband10pro` → `/data/canopus`；`game/pattern/rom.nes`，上限 64 KiB）；只支持 mapper 0（其他返回 `-1000 - id`）；`Emulator::init_in` 就地初始化 76,360 B 分配；像素指针指向 `Ppu::frame` 内部（零额外缓冲）；`step()` 以 `BUDGET_CYCLES=60,000` 为上限跑一帧。
- `frame.rs`：`NES_PALETTE_RGB565[64]`（`scripts/gen_palette.py` 生成）+ `convert_raw` 别名安全行式转换（栈上 128 列行缓冲 + `copy_nonoverlapping`）。
- `ui.rs`：`launcher_app_descriptor`（`app_id=0x4E45`、`package=com.canopus.nesband10pro`、图标 `/resource/app/launcher/compass.bin`）+ `firmware_page_descriptor`（`page_name=nes_player`）；`on_create` 建 `lvx_image_create(root)`；A/B 双 `ImageDsc`（24 B：magic 0x19、cf 0x12 RGB565、128×96、stride 256、data_size 24,576）+ `lvx_image_set_src`；50 ms `lvx_timer` 逐帧。
  - **本次修订**：删除硬编码 `VIEW_WIDTH=336` / `VIEW_HEIGHT=252` / `CONTENT_HEIGHT=424`，改为 `page_on_create` 时从 `get_resolution` 取真实分辨率（失败回退 336×480），`layout()` 计算整数倍缩放 `scale = min(w/128, (h-56)/96) ≥ 1`，居中 + 黑边。336×480 下为 2×（128×96 → 256×192，origin y=172）。

### 构建与打包（本次实测）
- `canopus build`：**PASS**，`14 sections, 311256 resident bytes, 0 undefined, 18700 relocs, 1 ctors, 1 dtors`，无告警。
- ELF sha256：`39ce46610c5db68061d8a0f4498a43f62c9decd1890b06db0503aaafc47cf97c`
- `canopus package` → `dist/nesband10pro-0.1.0.canopus`（661,504 B，1 target，versionCode 1），sha256 `9c42201373d3c234cc566ace79508b9326617d2005889a0680c6abfe730ea61f`。
- `canopus package verify` → `package OK`。
- 包结构：tar 含 `artifacts/xiaomi-band-10-pro-3.101.043/module.elf`、`manifest.json`、`receipts/...cmi`；manifest `target_id` / `target_pack_revision=8` 与 target pack 一致；无密码学签名，完整性由 SHA-256 + CMI receipt 保证。

### 主机侧验证
- `scripts/gen_test_rom.py` → `assets/pattern.nes`（24,592 B NROM 棋盘格，无版权内容），sha256 `aa2cf266a2953dbee9e3729f5edbc694c69d2148eed1a3e2d4c1d09e0f96b204`（两次运行可复现）。
- `probe/host-render`（复用设备端 `frame.rs`）：8 帧，`cycles/frame=29780`，`check=true`，`in-place conversion matches separate buffer: true`（源地址故意为奇数）；PNG 目视确认棋盘格正确。
- 按设备 `layout()` 参数合成 336×480 预览：2× 缩放，origin (40,172)，非黑边区域 256×192，目视无裁切。
- 内存实测：`Emulator` 76,360 + `Nrom` 49,168 + 块头 16 = **125,544 B**，相对 128 KiB 预算余量约 5.5 KiB。

### x-nes 本地补丁
- `patches/x-nes-043.patch`（5,889 B，sha256 `a4b3e3536297bd4c297190926bfaad047d62d073fd987d2974d029722cf33900`），12 文件 +63/−2：
  - `Cargo.toml` crate-type rlib
  - 6 处 `alloc` 导入（`common.rs` + 5 个 konami mapper）
  - 4 个 `init_in`（`Ppu`、`Apu`、`Bus`、`Emulator`）
  - `write_scroll` fine-Y 掩码 bug 修复（`0xFC1F` → `0x0C1F`）

### 文档与脚本
- `docs/02-implementation.md`（实现、内存核算、证据等级、补丁清单、复现命令、设备待验证项）
- `docs/DECISION.md`（按实际实现修订 D2/D4）
- `docs/03-verify.md`（阶段 3 主机侧验证报告）
- `REPORT.md`（最终交付报告）
- `scripts/build.sh`（一键复现：固定 commit 拉 vendor → 幂等打补丁 → build/package/verify → host-render + PNG → 打印 SHA-256）；**本次已端到端跑通，退出码 0，三个 sha256 与 REPORT.md 一致**

## 未完成 / 设备未验证

- `LV_COLOR_FORMAT_RGB565 = 0x12` 未被固件验证（`BUILD_ONLY`）。
- 模块堆内**变量图** descriptor 是否被接受。
- A/B descriptor 交替是否触发重绘。
- RGB565 字节序是否正确。
- `lvx_image_set_scale` 实际效果、整数倍缩放 + 黑边在真机上的观感。
- 真实帧率、`ctx->mem->used()` 余量在设备上是否与主机预测一致。
- 页面能否在启动器打开（`app_install` + `launcher_add` 两阶段是否被 UI heartbeat 正常推进）。
- 音频、按键输入、存档未做（本阶段不要求）。

## 阻塞原因

无硬阻塞。主要风险仍是 RGB565 变量图与 A/B 重绘触发；回退方案见 `docs/DECISION.md` D6（路径字符串轮转 / `lv_display_invalidate_area`）。

## 下次从这里继续

1. 读 `base/prompts/03-verify.md`（阶段 3）与 `docs/03-verify.md` 的待验证清单。
2. 设备侧按 `docs/02-implementation.md` 第 10 节 / `REPORT.md` 第 5 节逐项验证；优先确认 `cf=0x12`、字节序、descriptor 接受度、帧率与 `query` 的堆数据。
3. 若 RGB565 被拒：按 D6 回退到文件路径轮转，或换 ARGB8888 + 更小分辨率（注意堆余量仅 5.5 KiB，ARGB8888 128×96 = 49,152 B 不可行，需先裁 APU 或降分辨率）。
4. 用真机结果回填 `docs/03-verify.md`、`REPORT.md` 与 `STATE.md`，再决定是否进入阶段 4。

## 重要边界

- 所有固件地址只用 `targets/xiaomi-band-10-pro-3.101.043` 的已恢复记录；禁止跨 target 推断。
- 不提升任何符号的证据等级；不提交版权 ROM 或固件 bin。
- “编译通过 / 主机验证 / 设备验证”必须分开表述；当前**设备未验证**。
