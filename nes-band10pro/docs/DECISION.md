# DECISION — 架构与实现取舍

记录本任务中需要自主决策的问题、结论与依据。所有“固件地址/结构偏移”均属 STATIC_RECOVERED，未做设备验证。

## D1. 模块形态：target-bound（`canopus-target-private`）

**结论**：采用 target-bound Rust 模块，`Canopus.toml` 的 `[module] kind = "target"`，`[targets] include = ["xiaomi-band-10-pro-3.101.043"]`。

**依据**：

- 目标 loader 为 `nuttx-modlib-elf32-rel` + `symbol_imports = none`，模块必须自带全部实现并从 `init_array` 注册 descriptor；`canopus build` 会按 target 传入 `target-xiaomi-band-10-pro-3-101-043` feature。
- 逐帧像素替换需要 boot-resident LVGL 的 `lv_image_set_src` / `lv_image_set_scale` 与模块自建 `lv_image_dsc_t`，这些入口只在 target-private 门面（`band10_pro.rs`）中导出，portable 模块拿不到。
- portable（`canopus-runtime` / `canopus-ui-core`）的 UI ABI 单树 32 节点，适合静态控件，不适合每帧整体位图替换。

**风险**：target-bound 模块与固件 043 强绑定；跨固件不能复用（铁律：禁止跨 target 推断地址）。

## D2. 上屏路径：page 内容区 + `lv_image` + 自建 `lv_image_dsc_t`

**结论**：页面 `on_create` 中：

```
lvx_content_create(root) → set_size(336,424) → align(TOP_MID,0,56)
lvx_image_create(content) → lvx_image_set_src(image, dsc) → lvx_image_set_scale(image,336,336)
```

**依据**：见 `01-research.md` §4。`lv_image_dsc_t` 完全由模块静态分配（24 B × 2），像素缓冲为模块内 `static mut FB: [u16; 61440]`，不依赖固件内部结构或 `lv_canvas` 私有 API。

**刷新方式**：单帧缓冲 + 两个 descriptor 交替 `set_src`（descriptor 内容指向同一 `FB`）。目的不是真双缓冲，而是让 LVGL 每次都重新读取并重绘对象；`lv_display_invalidate_area` 未导出，不能手动失效区域。

**代价/风险**：缩放由固件 LVGL transform 完成，实际裁剪/对齐行为无法在主机复现，**设备未验证**。若设备上出现不刷新或错位，回退方案是：改为每次 `lvx_image_set_src(image, NULL)` 后再设回 descriptor，或缩小内容区为 256×240 直接 1:1 显示（黑边更多但无 transform）。

## D3. 帧率与每帧预算

**结论**：页面自有 LVGL 定时器周期 33 ms（≈30 fps），每次回调最多执行 `INSTRUCTION_BUDGET = 120_000` 条 CPU 指令，直到 `ppu.frame_complete` 或预算耗尽。

**依据与核算**：

- NTSC 单帧 29,780 CPU 周期；典型 NROM 平均 ~3 周期/指令，即每帧约 1 万条指令，120k 预算留出 ~12 倍余量应对慢指令/慢 mapper。
- 预算上限防止某帧卡死 UI 线程；超出预算的剩余周期留到下一次回调（等效跳帧）。
- 首帧在 `page_on_create` 内同步跑一次，保证页面不空白。
- 33 ms 下若单次回调耗时超过一帧时间会自然降帧；稳定优先，允许跳帧。

**未测**：设备上 120k 条指令的实际耗时（cortex-m33 @ 未知主频），**设备未验证**。若掉帧严重，下一步是把预算降到 40k–60k 并接受 15–20 fps，或把模拟放到独立线程（本轮范围外）。

## D4. 内存核算

模块动态堆由 `ctx->mem` 提供，约 128 KiB（`canopus-runtime::mem`）。主机侧实测：

| 项 | 字节 |
| --- | --- |
| `size_of::<nes::Emulator>()`（含 Bus/PPU 帧缓冲 61440） | 76,360 |
| mapper 自持 PRG（16 KiB 测试 ROM） | 16,384 |
| mapper 自持 CHR RAM | 8,192 |
| 合计稳态 | ≈ 100,936 |
| 余量（128 KiB = 131,072） | ≈ 30,136 |

峰值控制：`start_emulator` 在 `create_mapper()` 之后立即 `drop(rom)`，再构造 76 KiB 的 `Emulator`；否则 `Rom` 的 PRG/CHR 副本会与 Emulator 同时驻留，逼近上限。

**限制**：ROM 必须小。按当前设计（整 ROM 读入 + mapper 复制），约 24 KiB 的 PRG/CHR 总量是安全上限；更大的商业 ROM 会分配失败（`GlobalAlloc` 返回 null）。本轮只交付 16 KiB 自产 NROM 测试 ROM，符合“仅画面输出”的范围。后续若要跑更大 ROM，需要按 bank 流式读取或裁剪 x-nes 的 mapper 集合。

**风险**：`Emulator::new` 按值构造 76 KiB 结构，设备线程栈大小未知，有栈溢出风险（**设备未验证**）。若发生，下一步改为在静态存储上原地初始化 `Bus`。

## D5. ROM 策略

**结论**：

1. 优先读取 `/data/canopus/nespro.nes`（`ctx->fs` 的 `exists`/`stat`/`read_all`）。
2. 不存在或读取失败时，使用编译进模块的自产 16 KiB NROM 测试 ROM（`roms/nespro-test.nes`，`include_bytes!`）。

**依据**：TASKSPEC 要求“优先 `/data/canopus` 或模块 data 路径下的 `.nes`”；同时必须有一眼可见的画面证据。测试 ROM 由 `tools/make_test_rom.py` 生成，纯自产，无版权内容：

- mapper 0（NROM-128，16 KiB PRG 镜像到 `$8000`/`$C000`），CHR RAM（8 KiB，运行时由程序写入 4 个 tile）。
- 画面：4 组调色板 × 棋盘/纯色 tile 的循环图案（主机验证为 13 色）。
- 属性表 `0xE4/0x1B` 交替，写入后重置 `$2006 = $2000` 再开渲染（否则滚动地址错位导致取样错误）。

**不使用** nestest / 240p Test Suite 等第三方 ROM（许可证/体积/内存均不合适）。

## D6. x-nes 集成方式

**结论**：直接以 Rust crate（rlib）依赖集成，不用 `ffi` feature / C FFI。

**依据**：

- 模块本身就是 no_std Rust，静态链接最直接；`ffi` 只是把同一核心再包一层 C ABI，无收益。
- 链接证据：设备 ELF 内 887 个 `nes::` 符号、verifier `0 undefined`。
- vendor 补丁见 `01-research.md` §6；补丁只影响本地副本。

**代价**：x-nes 的 `Emulator` 固定含 61,440 B 帧缓冲与全 mapper 代码，无法按需裁剪；这是 D4 内存上限的主因。

## D7. 证据等级

| 项 | 等级 | 说明 |
| --- | --- | --- |
| `canopus build` + ELF verifier PASS | 编译通过 | 0 undefined、15 sections、1 ctor |
| `canopus package` + `package verify` | 编译通过（结构/哈希校验） | 非设备签名验证 |
| 主机侧 NES 帧（PPM/PNG） | 主机验证 | 与设备共用 `nes_frame.rs` 转换代码 |
| LVGL 上屏、页面注册、定时器、缩放 | 设备未验证 | 依赖 STATIC_RECOVERED 固件入口 |
| 固件地址/结构偏移 | 静态恢复（STATIC_RECOVERED） | 未提升证据等级 |
