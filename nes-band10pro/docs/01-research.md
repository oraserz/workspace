# 01 — 调研与基座

- 依据基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`（`https://github.com/oraserz/workspace.git`，本次 `pull --ff-only` 为 Already up to date）
- 基址副本：`/workspace/nes-band10pro/base/`（只读；`resource.bin` 位于其根目录）
- 第三方仓库（`/workspace/nes-band10pro/vendor/`）：
  - Canopus：`4928f1eb70594263292c32ffc531a7cad3aea555`（`feat(ui): extend native opacity support across v9 targets`）
  - x-nes：`ca4c3955b8a442fe25ee62abd57fca6849d1e70b`（`Update window.rs`）
- 本次执行时间：2026-10-27（Asia/Shanghai）
- 证据等级用语：**编译通过** / **主机验证** / **设备未验证**。下文所有结论均按此标注。

## 1. 目标固件与 target

来自 `vendor/canopus/targets/xiaomi-band-10-pro-3.101.043/target.toml`（target pack revision 8）：

| 项目 | 值 |
| --- | --- |
| target_id | `xiaomi-band-10-pro-3.101.043` |
| 架构 | armv8-m.main / cortex-m33 / thumb2 / 小端 / soft float |
| loader | `nuttx-modlib-elf32-rel`，`ET_REL`，构造发现 `init_array`，`symbol_imports = none` |
| module_abi | 1 |
| firmware_sha256 | `519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec` |
| 装载方式 | `insmod <path> <name>` / `rmmod <name>` |

这是 stock modlib 目标：模块不能依赖宿主导出符号（`symbol_imports = none`），只能通过 `ctx` 服务与 target-private 的固件地址门面工作（见 §4）。

## 2. 工具链与最小工程

`canopus doctor`（`CANOPUS_ROOT=/workspace/nes-band10pro/vendor/canopus`）结果：

```
ok    SDK          /workspace/nes-band10pro/vendor/canopus (CANOPUS_ROOT)
ok    targets      8 registered, 8 active: ... 3.101.043 ...
ok    C compiler   clang 17.0.0 (targets Cortex-M33)
ok    linker       LLD 17.0.0
ok    Rust         cargo 1.92.0; thumbv8m.main-none-eabi installed
ok    archiver     LLVM llvm-ar
ok    python3      Python 3.14.8
warn: targets/xiaomi-band-9-3.1.32 因 schema 校验失败被跳过（与本 target 无关）
```

`canopus build`（Rust 模块）的实际流水线（读自 `cli/canopus/src/project.rs`）：

1. `cargo build --release --target thumbv8m.main-none-eabi --no-default-features --features device,<dep>/target-<id>`
   （`RUSTFLAGS=-C target-cpu=cortex-m33`、`CARGO_PROFILE_RELEASE_PANIC=abort`、`OPT_LEVEL=z`、`LTO=thin`、`CODEGEN_UNITS=1`）
2. 取唯一 `staticlib` `.a`，编译 `sdk/rust/c_shim/canopus_ctor.c`（通过 `CANOPUS_MODULE_ENTRY` 注册 descriptor）
3. `ld.lld -r -T sdk/rust/c_shim/module.ld` 生成 `build/<target>/<id>.elf`
4. 运行 ELF verifier：要求 0 undefined、`init_array` 中有 1 个构造函数、target_id/firmware 哈希匹配

最小工程验证并入 `nespro` 本身完成（没有另建空模板）：`canopus build` → verifier PASS，`canopus package --build` → `.canopus`，`canopus package verify` → OK。**编译通过**。

## 3. x-nes 交叉编译与链接

- 版本 0.1.3，edition 2024，`no_std`（`#![cfg_attr(not(any(test, feature = "std")), no_std)]`）。
- 顶层入口：`nes::Emulator { cpu, bus }`，`Emulator::new(mapper)`、`emu.tick() -> u8`；`nes::rom::Rom::new(&[u8])`、`rom.create_mapper()`；帧缓冲 `emu.bus.ppu.frame: [u8; 61440]`（256×240 调色板索引），帧结束标志 `emu.bus.ppu.frame_complete`。
- crate 的 lib 名为 `nes`（不是 `x-nes`），Rust 侧 `use nes::...`。
- 交叉编译验证：曾直接对 `thumbv8m.main-none-eabi --no-default-features` 编译，出现 3 类问题并已在 vendor 副本修复（详见 §6）：`Vec`/`vec!` 在 no_std 下缺 `alloc` 导入、库内无 allocator/panic handler。
- 链接验证：设备 ELF 内 `llvm-nm` 可数出 402 个 `_ZN3nes...` 符号（x-nes 模块/类型），verifier 报 `0 undefined`。**编译通过**，说明 x-nes 目标代码确实进入产物。

## 4. 上屏路径调研

Band 10 Pro 的屏幕与页面布局来自 target-private 门面（`sdk/rust/canopus-target-private/src/band10_pro.rs`，STATIC_RECOVERED）：

- `CONTENT_WIDTH = 336`、`CONTENT_HEIGHT = 424`、`CONTENT_TOP_OFFSET = 56`（`ALIGN_TOP_MID`），即整屏 336×480，页内容区在状态栏之下 336×424。
- 原生应用通过 `ctx->launcher->register_app(AppEntryV1)` 注册页面；页面 `on_create(page, root, start_data)` 得到 LVGL root，然后：
  - `lvx_content_create(root)` 建内容容器并 `set_size(336,424)` + `align(TOP_MID,0,56)`；
  - `lvx_image_create(content)` 建图像控件；
  - `lvx_image_set_src(image, dsc)` 设置模块自建的 `lv_image_dsc_t`；
  - `lvx_image_set_scale(image, 336, 336)`（LVGL v9 单位为 1/256，336/256≈1.3125，256→336、240→315）；
  - `lvx_timer_create(cb, 33, NULL)` 建页面自有周期定时器，`lvx_timer_delete` 销毁。

`lv_image_dsc_t`（LVGL v9.2.0）布局 24 字节：`lv_image_header_t`（magic:8、cf:8、flags:16、w:16、h:16、stride:16、reserved:16）+ `data_size:u32` + `data:*const u8` + `reserved:*const void`。magic `0x19`，RGB565 颜色格式 `0x12`，stride = width×2。

Canopus UI ABI（`canopus-runtime` 的 IMAGE/VISUAL/CAPTURE，单树 32 节点）适合静态界面，不适合每帧整体替换像素；target-bound 直接使用 boot-resident LVGL `lv_image` 是更低风险路径。决策依据见 `DECISION.md`。

固件私有 LVGL 入口在 `canopus_veneer.h` 中标注 restricted，`lv_display_invalidate_area` 为 STATIC_RECOVERED 且未导出，因此刷新完全依赖 `lv_image_set_src` 触发对象重绘。**证据等级：静态恢复（STATIC_RECOVERED），设备未验证。**

### 屏幕参数的运行时校验

`nespro` 在 `page_on_create` 里调用 `ctx->display->get_resolution()` 并在日志中输出“display 336x480”或“display resolution differs from 336x480”，避免把尺寸当成不可质疑的常量。主机侧无法执行此分支，**设备未验证**。

## 5. `resource.bin`（MC 安装表盘）分析

- 文件：`/workspace/nes-band10pro/base/resource.bin`，784729 字节，头 16 字节 `5a a5 34 12 03 07 ...`（Magic + 类型/版本 + 长度字段）。
- 用 Mi8WfBinTool 解包（`java Mi8WfBinTool unpack resource.bin ./out mi8pro`，10 Pro 归 `mi8pro` 规范），产物在 `/workspace/nes-band10pro/analysis/mi8/out/`：
  - `wfDef.json`（1,529,108 B）：`faceStyleCount = 6`，每个 face 有 `imageDir`、`elements`、`imageDefs`、`imageListDefs`、`extraJs`；元素类型含 `type_element_lua`。
  - `images/preview.png`：336×480 RGBA，与 §4 的整屏尺寸一致。
  - `images/lua/main.lua`（11,481 B）、`images/lua/game.elf`（726,540 B，ARM ELF，表盘内嵌游戏二进制）、`images/lua/grass.png`（37,702 B）。
- 结论：表盘资源是“Lua 脚本 + ARM ELF + PNG 图元”的组合，整屏 336×480；图元为索引色/压缩位图，按控件布局贴图。对 NES 上屏的启示是：**整帧位图 + 固定区域刷新**是可行的，但表盘使用的私有运行时不适用于模块；模块应走 §4 的 LVGL 路径。
- **不得复用其美术资源发布**。`analysis/mi8/out/` 仅为本地分析证据，不属于交付物。

## 6. vendor 本地补丁（x-nes）

为了让 x-nes 在 `thumbv8m.main-none-eabi --no-default-features` 下可作为 rlib 依赖编译：

1. `Cargo.toml`：`rust-version = "1.97.0"` → `"1.85"`（本机 rustc 1.92 可编译；上游要求更高但不使用新特性）。
2. `Cargo.toml`：`[lib] crate-type = ["lib", "cdylib", "staticlib"]` → `["lib"]`。否则 cargo 会为设备三元组同时构建 staticlib/cdylib 变体，而库本身没有 allocator/panic handler，直接编译失败。
3. `src/mapper/konami/{vrc1,vrc2_4,vrc3,vrc6,vrc7}.rs`：补 `use alloc::vec::Vec;`（10 处 E0412）。
4. `src/mapper/common.rs`：补 `use alloc::vec;`（`vec![0; bank_size]` 宏 6 处）。

这些改动只影响本地 vendor 副本，不向任何上游仓库推送。

## 7. 结论

- 目标固件、工具链、模块流水线均已确认可用，`nespro` 已能 **编译通过** 并通过 ELF verifier。
- 上屏路径确定为 target-bound LVGL `lv_image` + 模块自建 `lv_image_dsc_t`。
- x-nes 已真正链接进 ELF，NES 帧在主机侧可正确渲染。
- 所有固件地址/结构偏移均为 STATIC_RECOVERED，**设备未验证**；本报告不声称设备可运行。
