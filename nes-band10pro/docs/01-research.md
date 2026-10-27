# 阶段 1 调研记录

执行日期：2026-10-27（CST）
基址仓库：https://github.com/oraserz/workspace.git
基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`

## 依赖仓库（只读 vendor）

| 仓库 | commit |
| --- | --- |
| Canopus | `4928f1eb70594263292c32ffc531a7cad3aea555` |
| Canopus-App-LyraPlayer | `8b228e37157766e0ea7e3d31a9d0ca2aaf053d7f` |
| x-nes | `ca4c3955b8a442fe25ee62abd57fca6849d1e70b` |
| Mi8WfBinTool | `5e4195ce91b9d8810a7a2233c96b4cba44184adc` |

## 工具链

- `rustc 1.97.0 (2d8144b78 2026-07-07)`，target `thumbv8m.main-none-eabi`（x-nes 要求 1.97+）。
- 工作目录固定 `rust-toolchain.toml`（channel 1.97.0 + 上述 target），避免默认工具链漂移。
- `canopus 0.1.0`（`vendor/Canopus/target/release/canopus`，需要 `CANOPUS_ROOT`）。

## target 事实（043）

- `xiaomi-band-10-pro-3.101.043/target.toml`：armv8-m.main / cortex-m33 / thumb2 / soft float；loader `nuttx-modlib-elf32-rel`；module_abi 1；relocation_profile `best1503-v1`；revision 8；`max_size = 0`；firmware sha256 `51930767...56ec`；地址范围 `0x0C000000 + 0x00E00000`、`0x1C000000 + 0x00080500`。
- 模块是 ET_REL ELF32 ARM，Thumb-2、软浮点，`init_array` 发现构造函数，0 符号导入。
- ELF verifier（CAN-P1-011）扫描模块内出现的固件地址，必须落在 target symbols 的 entry+callable 允许表内。

## `canopus build` 的实际行为（Rust 模块）

`cli/canopus/src/project.rs::compile_rust`：

```
cargo build --release --target thumbv8m.main-none-eabi --no-default-features \
  --features device[,target-<id 点换横线>][,canopus-target-generated/target-...][,canopus-target-private/target-...]
```

- 环境：`CANOPUS_TARGET_ID`、`CC_thumbv8m_main_none_eabi=clang`、`AR_...=llvm-ar`、`CFLAGS_...=--target=arm-none-eabi -mcpu=cortex-m33 -mthumb -mfloat-abi=soft`、`RUSTFLAGS=-C target-cpu=cortex-m33`。
- 要求 `[build] sources = ["Cargo.toml"]`，release 目录下恰好一个 `.a`。
- 链接时自动加入 `sdk/rust/c_shim/canopus_ctor.c`（`CANOPUS_MODULE_ENTRY(canopus_module_descriptor)` + destructor 调 `canopus_mod_stop(0)`），因此模块**不需要**自己实现 `canopus_register_module_descriptor`。
- `canopus package` 写 `dist/<id>-<version>.canopus`（POSIX tar：manifest + 每 target 的 ELF 与 CMI2 收据 + resources）。

## x-nes 事实

- crate 名 `x-nes`，库名 `nes`，edition 2024，`extern crate alloc`，`no_std`（非 std/test）。
- README 宣称的 `ffi` feature 与 `nes_*` C API **在当前源码中不存在**，只能直接使用 Rust API。
- `Emulator { cpu, bus }`；`tick()` 单步；`Ppu.frame: [u8; 61440]` 为 256×240 调色板索引；`frame_complete` 只置位、不清除。
- `Ppu::new`/`Bus::new`/`Nrom::new` 会在栈上构造大临时值（Ppu 约 60 KiB），而 `insmod` 任务栈只有 7.9 KiB，因此必须改造成“就地初始化 + 大数组上堆”。
- `Mapper(Box<dyn MapperImpl>)` 动态分发，`Mapper::from_ines` 才做 ROM 解析；未知 mapper 回退 `Nrom`。

## 上屏路径比较

| 路径 | 能力 | 证据等级 | 结论 |
| --- | --- | --- | --- |
| Canopus UI ABI 2.0（`ctx->ui`，语义树 ≤32 节点，`CANOPUS_UI_NODE_IMAGE` 只接受 `resource_id` + 路径） | 不能传内存帧缓冲 | ABI 文档 + 生成绑定 | 不适合逐帧画面 |
| `canopus-target-private::band10_pro` 的 `lvx_image_create` / `lvx_image_set_src` / `lvx_object_*` / `lvx_timer_create` | 可建 LVGL 对象、指针源、定时刷新 | `lv_image_create`、`lv_image_set_src` 为 DEVICE_PROVEN（LyraPlayer，仅文件路径源）；lvx 其余包装为 target-private | 采用；内存 `lv_image_dsc_t` 源属新增用法，设备未验证 |
| `lv_display_invalidate_area` | 直接触发重绘 | 043 generated 只有 CALLABLE 常量，private 未导出 wrapper | 不可用，改用双描述符交替触发失效 |

`lv_image_set_src` 对相同指针可能短路，因此模块用两个 `lv_image_dsc_t` 静态描述符交替指向同一 RGB565 缓冲，强制每次都是“新源”。

## 屏幕参数（不凭记忆）

- 运行时：`ctx->display->get_resolution(ctx, &w, &h)`；失败/异常值（0 或 >4096）回退 336×480。
- 解包 `resource.bin` 得到的 `images/preview.png` 尺寸为 **336×480**，与 LyraPlayer `PlayerLayout::BAND_10_PRO`（336×480，内容高 424、顶部 56）一致。
- 256×240 画面 1:1 居中：偏移 `x=(336-256)/2=40`、`y=(480-240)/2=120`，不缩放。

## resource.bin 分析（基址根目录，784729 字节）

- 工具：`Mi8WfBinTool`（`javac` + `java Mi8WfBinTool unpack resource.bin ./out mi8pro`）。
- 该工具在本样本上写 `images/lua/*.lua|elf` 前不会创建父目录，直接运行会 `NoSuchFileException`；本地在 `/tmp` 的副本里给两处 `Files.write` 加了 `createDirectories(parent)` 后解包成功（未修改 vendor 仓库）。
- 解包结果：
  - `wfDef.json`：表盘名 `Minecraft`，`faceStyleCount = 6`，单 face 的 `elements` 只有一个 `type_element_lua`（`lua/main.lua`）。
  - `images/preview.png`：336×480，8-bit RGBA（color type 6）。
  - `images/lua/grass.png`：192×192，8-bit RGBA。
  - `images/lua/main.lua`：11481 字节安装脚本；`EXPECT_FW='3.101.043'`、`EXPECT_BUILD=825192154`、`EXPECT_BYTES=726540`，会把 726540 字节的原生 ELF 安装到 `/data/minecraft10-073.elf`，状态节点 `/dev/minecraft10_073`。
  - `images/lua/game.elf`：726540 字节，ELF32 / ARM（e_machine 0x28）/ ET_REL。
- 结论：该样本是“Lua 安装器 + 原生 ELF + PNG 资源”的 MC 表盘，工具层把图元解码成 PNG（RGBA），没有暴露自定义索引色/RLE 细节可供复用；可用于确认面板尺寸 336×480 与“原生 ELF 走 `/data`”的组织方式，美术资源不复用。

## 证据等级用词

- **DEVICE_PROVEN**：目标固件上真机验证过的符号/行为（仅沿用 vendor 记录，本轮未新增）。
- **STATIC_RECOVERED**：静态恢复/生成绑定，未真机验证。
- **host/static evidence**：主机测试或静态分析支持，未上设备。
- 本任务所有画面结论均为 **主机验证**，不等于设备验证。
