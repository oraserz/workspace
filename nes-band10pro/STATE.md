# STATE

- 依据的基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`（`https://github.com/oraserz/workspace.git`，已 `pull --ff-only`，未推送）
- 上次执行时间：2026-10-27 02:15 CST（2026-10-26T18:15Z）
- 当前阶段：**阶段 2 已完成（构建 + 打包 + 主机侧验证）→ 阶段 3（验证）待开始**
- 已完成：
  - 阶段 1 全部内容（工具链、最小工程闭环、x-nes 交叉编译与 ET_REL 链接、`resource.bin` 解包与格式分析、`docs/01-research.md`、`docs/DECISION.md`）。
  - 阶段 2 正式模块 `nesband10pro/`（target-bound Rust，`staticlib`，no_std）：
    - `lib.rs`：`prepare` 只绑 context（7.9 KiB 栈、零分配）；`activate` 里做全部初始化；`GlobalAlloc` 走 `ctx->mem`；`query` 暴露 9 个 u32（含 `heap_used/available`）；两阶段 `publish_native_app_stage`。
    - `nes.rs`：ROM 查找（模块 data 路径 → `/data/canopus/nesband10pro` → `/data/canopus`；`game/pattern/rom.nes`，上限 64 KiB）；只支持 mapper 0（其他返回 `-1000 - id`）；`Emulator::init_in` 就地初始化 76,360 B 分配；**像素指针指向 `Ppu::frame` 内部**（零额外缓冲）；`step()` 以 `BUDGET_CYCLES=60,000` 为上限跑一帧。
    - `frame.rs`：`NES_PALETTE_RGB565[64]`（由 `scripts/gen_palette.py` 生成）+ `convert_raw` 别名安全行式转换（栈上 128 列行缓冲 + `copy_nonoverlapping`）。
    - `ui.rs`：`launcher_app_descriptor`（`app_id=0x4E45`、`package=com.canopus.nesband10pro`、图标 `/resource/app/launcher/compass.bin`）+ `firmware_page_descriptor`（`page_name=nes_player`）；`on_create` 建 `lvx_image_create(root)`；A/B 双 `ImageDsc`（magic 0x19、cf 0x12 RGB565、128×96、stride 256、data_size 24,576）+ `lvx_image_set_src`；`lvx_image_set_scale(672)` 放大 2.625× 到 336×252；50 ms `lvx_timer` 逐帧。
  - `canopus build`：**PASS**，`14 sections, 311220 resident bytes, 0 undefined, 18700 relocs, 1 ctors, 1 dtors`，ELF sha256 `136de7b1cdc138a674a1f72f5dda7a69269e55591b9b10840023a1ab5186deb1`，无编译告警。
  - `canopus package`：`dist/nesband10pro-0.1.0.canopus`（661,504 B，1 target，versionCode 1）。
  - 测试 ROM：`scripts/gen_test_rom.py` 生成 `assets/pattern.nes`（24,592 B NROM 棋盘格，无版权内容）。
  - 主机侧验证 `probe/host-render`（复用设备端 `frame.rs`）：8 帧，`cycles/frame=29780`，`check=true`（32×32 块颜色循环正确、从 `y=0` 起），`in-place conversion matches separate buffer: true`（源地址故意为奇数）；PNG 已目视确认。
  - 内存实测：`Emulator` 76,360 + `Nrom` 49,168 + 块头 16 = **125,544 B**，相对 128 KiB 预算余量约 5.5 KiB。
  - x-nes 本地补丁（12 文件，+76/−2）：rlib crate-type、6 处 `alloc` 导入、4 个 `init_in`、`write_scroll` fine-Y 掩码 bug 修复（`0xFC1F` → `0x0C1F`）。
  - 产出 `docs/02-implementation.md`（实现、内存核算、证据等级、补丁清单、复现命令、设备待验证项），并按实际实现修订 `docs/DECISION.md` D4。
- 未完成 / 设备未验证：
  - `LV_COLOR_FORMAT_RGB565 = 0x12` 未被固件验证（`BUILD_ONLY`）。
  - 模块堆内**变量图** descriptor 是否被接受、A/B descriptor 交替是否触发重绘、RGB565 字节序、`lvx_image_set_scale` 实际效果、真实帧率与堆余量。
  - 页面能否在启动器打开（`app_install` + `launcher_add` 两阶段是否被 UI heartbeat 正常推进）。
  - 未调用 `ctx->display->get_resolution`（当前用 336×480 常量）；音频、按键、存档未做。
- 阻塞原因：无硬阻塞。主要风险仍是 RGB565 变量图与 A/B 重绘触发；回退方案见 `docs/DECISION.md` D6（路径字符串轮转 / `lv_display_invalidate_area`）。
- 下次从这里继续：
  1. 读 `base/6adf7b7fdcdc524bcccab5e8_03-verify.md`（阶段 3）。
  2. 设备侧按 `docs/02-implementation.md` 第 10 节逐项验证；优先确认 `cf=0x12`、字节序、descriptor 接受度、帧率与 `query` 的堆数据。
  3. 若 RGB565 被拒：按 D6 回退到文件路径轮转或换 ARGB8888 + 更小分辨率（注意堆余量仅 5.5 KiB，ARGB8888 128×96 = 49,152 B 不可行，需先裁 APU 或降分辨率）。
  4. 补 `ctx->display->get_resolution` 运行时复核，把 336/424 常量改成派生值。
  5. 用真机结果回填 `docs/03-verify.md` 与 `STATE.md`，再决定是否进入阶段 4。
- 重要边界：
  - 所有固件地址只用 `targets/xiaomi-band-10-pro-3.101.043` 的已恢复记录；禁止跨 target 推断。
  - 不提升任何符号的证据等级；不提交版权 ROM 或固件 bin。
  - “编译通过 / 主机验证 / 设备验证”必须分开表述；当前**设备未验证**。
