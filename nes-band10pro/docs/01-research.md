# 阶段 1 调研报告：小米手环 10 Pro（043）NES 上屏

- 执行日期：2026-10-27（Asia/Shanghai）
- 基址仓库：`https://github.com/oraserz/workspace.git`
- 基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`（已 `pull --ff-only`，未推送）
- 目标：`xiaomi-band-10-pro-3.101.043`（armv8-m.main / Cortex-M33 / Thumb2 / soft float）
- 本轮范围：只做“把 NES 帧输出到手环屏幕”的可行性基座；音频、按键、存档后置。

> 证据等级约定（沿用 Canopus）：`STATIC_CONFIRMED` > `STATIC_RECOVERED` > `DEVICE_PROVEN`（本项目内指 Canopus 维护者在真机验证过的能力）> `HOST_TESTED` > `BUILD_ONLY`。
> 本报告中的“编译通过 / 链接通过”一律不等于设备验证。

---

## 1. 结论摘要

1. **可以做，但上屏不能走 Canopus 语义 UI 的 IMAGE 节点逐帧刷新。** `ctx->ui` 的 IMAGE 节点只接受“图片路径字符串 + resource_id”，由 UI host 调用 `lv_image_set_src(image, path)`；每次提交走 32 节点语义树重建，且图片按路径解码/缓存，无法承载 20–30fps 的内存帧缓冲。
2. **可行主路径是 target-bound 原生页面 + 已恢复 LVGL 绑定**：模块用 `ctx->launcher->register_app` 的 native page（或原厂 `app_install`）拿到 `on_create(page, root, ...)` 的 LVGL root 指针，`lvx_image_create(root)` 建一个 image widget，再用**模块自有内存中的 LVGL v9 image descriptor**（`lv_image_set_src`，DEVICE_PROVEN）逐帧换源，`lvx_image_set_scale`（256=100%）做整数倍放大。LyraPlayer 已在同一 target 用 `lvx_image_create` + `lvx_image_set_src` + `lvx_image_set_scale` 跑通原生页面与图片路径，可作为工程范式（不照抄业务逻辑）。
3. **工具链闭环已跑通**：`canopus doctor` 除 Rust 默认工具链提示外全 ok；`canopus new` → `canopus build`（verifier PASS）→ `canopus package` 产出 12,288 字节 `.canopus`；x-nes 已用 Rust 1.97.0 交叉编译为 `thumbv8m.main-none-eabi` rlib，并与 C stub 用 `ld.lld -m armelf -r` 完成最小可重定位链接（0 undefined）。
4. **内存是硬约束**：`Emulator` 结构体实测 76,360 字节（其中 PPU 65,880、APU 8,368、CPU 8），x-nes 的 256×240 帧缓冲 61,440 字节就在 PPU 内。模块动态堆按 TASKSPEC 的 ~128K 预算时，ROM(24K, NROM 16K PRG + 8K CHR) + Emulator(76.4K) + 显示缓冲必须控制在 ~27K 内；因此显示缓冲选 **128×96 RGB565（24,576 字节）**，再用 LVGL scale=512 放大到 256×192 居中显示。若设备实际堆更大（Canopus 2026-10-19 记录基线已扩到 256 KiB，但 `.043` 固件是否放行未被本任务证据确认），可升到 160×112 或 160×120。
5. **resource.bin 是 Minecraft 原生安装表盘**：容器魔数 `0x1234A55A`，含 `wfDef.json`、`preview.png`(336×480)、`lua/main.lua`(安装壳)、`lua/game.elf`(ELF32 ARM ET_REL，726,540 字节)、`lua/grass.png`(192×192)。该样本 `imageDefs=0`，**没有实际 RLE 图元**；RLE/索引色格式来自 Mi8WfBinTool 的 `mi8pro` 编解码实现，只能作为“格式知识”，不能声称被本样本验证。
6. **屏幕尺寸**：解包 `preview.png` 为 336×480；运行时仍以 `ctx->display->get_resolution` 为准（TASKSPEC 要求，不写死）。

---

## 2. 环境、日期守卫与锁

- 本地日期 `2026-10-27`，符合执行窗口；未新建任何定时任务。
- `.run.lock` 在本次开始时由本会话写入（`2026-10-26T16:33:50Z` UTC，对应本地 2026-10-27 00:33），未超过 6 小时，属同一执行，未跳过。
- 主机：Linux x86_64 沙箱；无 TTY，非交互执行。

| 工具 | 版本 | 备注 |
|---|---|---|
| clang | 17.0.0 (swiftlang) | `canopus doctor` 识别为 Cortex-M33 编译器 |
| LLD | 17.0.0 | `ld.lld -m armelf -r` 用于 ET_REL 合并 |
| llvm-ar / llvm-nm / llvm-size / llvm-readelf | 17.0.0 | 静态库与 ELF 检查 |
| rustc / cargo | 1.97.0 | `+1.97.0` 显式调用；已 `rustup target add thumbv8m.main-none-eabi` |
| Java | OpenJDK 25.0.4.1 LTS | 运行 Mi8WfBinTool |
| Python | 3.14.8 | 解包分析与文档脚本 |
| canopus CLI | 0.1.0（源码构建，`vendor/Canopus/target/release/canopus`） | 通过 `tools/bin/canopus` 符号链接使用 |

`canopus doctor` 实测（工作目录 `probe/hello`）：

```
ok    SDK          /workspace/nes-band10pro/vendor/Canopus (checkout)
ok    targets      9 registered, 9 active: 3.100.043 3.1.28 3.101.036 3.101.043 ...
ok    C compiler   clang version 17.0.0 ...; targets Cortex-M33
ok    linker       LLD 17.0.0 ...
warn  Rust         cargo 1.92.0 ...; Rust modules need `rustup target add thumbv8m.main-none-eabi`
ok    archiver     LLVM ...
ok    curl         curl 8.5.0 ...
ok    python3      Python 3.14.8
```

> Rust 的 warn 是因为默认工具链是 1.92.0；本项目统一用 `cargo +1.97.0`，且 1.97.0 已装 `thumbv8m.main-none-eabi`，不影响构建。

---

## 3. 第三方仓库与 commit

全部克隆在 `/workspace/nes-band10pro/vendor/`，只读基址在 `/workspace/nes-band10pro/base/`。

| 仓库 | commit | 用途 |
|---|---|---|
| Canopus | `4928f1eb70594263292c32ffc531a7cad3aea555` | 框架、target 定义、SDK、verifier、CLI |
| Canopus-App-LyraPlayer | `8b228e37157766e0ea7e3d31a9d0ca2aaf053d7f` | 同 target 原生页面/图片/构建范式参考 |
| x-nes | `ca4c3955b8a442fe25ee62abd57fca6849d1e70b` | NES 核心 |
| Mi8WfBinTool | `5e4195ce91b9d8810a7a2233c96b4cba44184adc` | `resource.bin` 解包与格式参考 |

- 基址副本文件名带前缀（如 `6adf7b7fdcdc524bcccab5e8_TASKSPEC.md`），内容与 TASKSPEC/prompts 对应；`resource.bin` 位于基址根目录（784,729 字节）。
- 未向基址仓库推送任何内容；基座仓库只读，构建全部在工作目录内完成。

---

## 4. 最小工程闭环（canopus new / build / package）

工程：`/workspace/nes-band10pro/probe/hello/`（C 模板，最小模块）。

```bash
cd /workspace/nes-band10pro/probe/hello
../../tools/bin/canopus new hello --target xiaomi-band-10-pro-3.101.043   # 已生成
../../tools/bin/canopus build
../../tools/bin/canopus package
```

实测输出：

```
built ./build/xiaomi-band-10-pro-3.101.043/hello.elf
sha256: 2edb13c9101c9a7dbbc383efeae0b62897691ffe6a41a969fbef88bfde78a6cb
summary: PASS (69 sections, 921 resident bytes, 0 undefined, 34 relocs, 1 ctors, 0 dtors)
ok: hello built and verified for 1 target(s)

wrote ./dist/hello-0.1.0.canopus (12288 bytes, 1 target(s), versionCode 1)
```

- 产物绝对路径：`/workspace/nes-band10pro/probe/hello/build/xiaomi-band-10-pro-3.101.043/hello.elf`、`/workspace/nes-band10pro/probe/hello/dist/hello-0.1.0.canopus`。
- 结论：target `xiaomi-band-10-pro-3.101.043` 的模块 ABI、链接脚本、verifier、打包链路可用。证据等级：`BUILD_ONLY`（未上设备）。

---

## 5. x-nes 交叉编译与最小链接验证

### 5.1 对 vendored x-nes 的必要补丁（共 5 类）

x-nes 上游默认按 `std` 库 + 多 crate-type 发布，直接作为 no_std 依赖会失败。对 `vendor/x-nes` 做了最小修改，全部记录如下：

1. `Cargo.toml`：`[lib] crate-type = ["rlib"]`（原 `["lib","cdylib","staticlib"]`）。staticlib 会把“必须提供 global allocator / panic handler”的责任压到 rlib 内部，导致下游无法提供。
2. `src/mapper/common.rs`：补 `use alloc::vec;`（`vec![]` 宏在 no_std 下不可见）。
3. `src/mapper/konami/vrc1.rs`、`vrc2_4.rs`、`vrc3.rs`、`vrc6.rs`、`vrc7.rs`：各补 `use alloc::vec::Vec;`（字段用 `Vec<u8>` 但未导入）。

> 这些是构建可行性补丁，不改变模拟语义。上游若后续修复，可平滑回退。

首次交叉编译还遇到 `no global memory allocator found` / `#[panic_handler] required`，由下游探针工程提供（见 5.2），不是 x-nes 缺陷。

### 5.2 探针工程与最小链接

目录：`/workspace/nes-band10pro/probe/xnes-probe/`

- `Cargo.toml`：`[lib] crate-type=["staticlib"]`，依赖 `x-nes = { path="../../vendor/x-nes", default-features=false }`，`panic="abort"`，`opt-level="z"`，LTO。
- `src/lib.rs`：`#![no_std]` + `extern crate alloc`；实现 `BumpAlloc`（静态 192KiB 探针堆，仅用于链接验证，非产品内存方案）、`#[panic_handler]`；导出：
  - `xnes_probe_boot(const u8* data, usize len) -> i32`
  - `xnes_probe_frame() -> const u8*`（循环 `emulator.tick()` 直到 `bus.ppu.frame_complete`，返回 `bus.ppu.frame` 指针，256×240 调色板索引）
- `link/probe_stub.c`：C 侧引用上述两个符号，保证 `ld.lld` 只拉取可达对象，模拟真实模块的 C/Rust 混合链接形态。

命令与结果：

```bash
cd /workspace/nes-band10pro/probe/xnes-probe
cargo +1.97.0 build --release --target thumbv8m.main-none-eabi
# -> target/thumbv8m.main-none-eabi/release/libxnes_probe.a (6,252,196 字节)

clang --target=thumbv8m.main-none-eabi -mcpu=cortex-m33 -mfloat-abi=soft -mthumb \
      -O2 -c link/probe_stub.c -o link/probe_stub.o
ld.lld -m armelf -r -o link/module_probe.o link/probe_stub.o \
      target/thumbv8m.main-none-eabi/release/libxnes_probe.a
# -> LINK OK，llvm-nm -u 无 undefined
llvm-size link/module_probe.o
# text 283518 / data 76344 / bss 196612
```

- 链接形态验证通过：Rust rlib + C stub → 单个 ET_REL，0 undefined，无 SVC，符合 Canopus `nuttx-modlib-elf32-rel` 加载模型的形状要求（最终模块仍需 `canopus build` 的 verifier 判定）。
- 仍有一条 `unused_must_use`（`emulator.tick()` 返回值未使用），阶段 2 改为 `let _ = ...` 即可。
- 证据等级：`BUILD_ONLY`（交叉编译 + 链接，不是设备验证，也未跑帧）。

### 5.3 x-nes 内存实测（host，Rust 1.97.0）

工具：`/workspace/nes-band10pro/probe/sizecheck`（host 小工具，仅打印 `size_of`）。

```
Emulator      76360
  Cpu             8
  Bus         76352
    Ppu       65880      # 含 frame: [u8; 61440]
    Apu        8368
    Mapper       16      # Box<dyn Mapper> 胖指针
Rom               56      # Vec 头，不含堆上的 PRG/CHR
```

- 关键 API（源码确认）：`Rom::new(&[u8]) -> Result<Rom, RomError>`、`rom.create_mapper() -> Mapper`、`Emulator::new(mapper)`、`emulator.tick() -> u8`、`emulator.bus.ppu.frame: [u8; 61440]`、`emulator.bus.ppu.frame_complete: bool`。
- `Rom::new` 会把 PRG/CHR `to_vec()` 进堆；`create_mapper()` 又会让各 mapper 再复制一份。因此**阶段 2 必须控制峰值**：先建 mapper、释放 `Rom`，再分配 Emulator 与显示缓冲；NROM 测试 ROM 只保留 16KiB PRG + 8KiB CHR。
- mapper 支持面：x-nes 覆盖 NROM/MMC1/MMC3/UNROM/CNROM/AOROM、VRC 系列、Namco 等；本任务只承诺 **mapper 0 (NROM)**，其余 mapper 需在内存/时间预算允许时再开。

---

## 6. resource.bin 解包与格式分析

### 6.1 容器与解包

```bash
# Mi8WfBinTool（Java，单文件）
javac Mi8WfBinTool.java
mkdir -p build/resource_unpacked/images/lua     # 工具不会自建该目录
java Mi8WfBinTool unpack /workspace/nes-band10pro/base/resource.bin \
     /workspace/nes-band10pro/build/resource_unpacked mi8pro
```

- `resource.bin`：784,729 字节；文件头 `5A A5 34 12`（LE `0x1234A55A`），版本字段 `03 07`，末尾以 PNG `IEND` 结束。
- 解包产物：

| 文件 | 大小 | 说明 |
|---|---|---|
| `wfDef.json` | 1,529,108 | 表盘定义；`name=Minecraft`、`id=minecraft`、`faceStyleCount=6`，但实际 `faces` 只有 1 个 `Minecraft` |
| `images/preview.png` | 5,020 | 336×480，表盘预览 |
| `images/lua/main.lua` | 11,481 | 安装壳（Lua） |
| `images/lua/game.elf` | 726,540 | 原生游戏模块 |
| `images/lua/grass.png` | 37,702 | 192×192 贴图 |

- `wfDef.json` 的 `faces[0].elements` 只有 1 个 `type_element_lua`（`lua/main.lua`）；`imageDefs=[]`、`imageListDefs=[]`；`extraJs` 内嵌 `lua/game.elf` 的十六进制原文（所以 JSON 才 1.5MB）。**该样本没有任何 RLE 图片记录**。

### 6.2 mi8pro 图片编码（来自 Mi8WfBinTool 实现，非本样本实测）

- 图片块头：`sign@+0`、`count@+1`、`flags@+2`、`width u16@+4`、`height u16@+6`、`length u32@+8`，像素从 `+12` 开始。
- 压缩载荷头：`E0 21 A5 5A` + `u32 info`（`raw_len = info >> 4`，`bpp = info & 0xF`）+ 压缩流。
- `mi8pro` 的 sign 取 `0x10` 时使用 **RLE V2.0**：
  - 控制字节 `c`；`n = c & 0x7F`；
  - `c & 0x80 != 0`：后接 `n` 个字面字节；
  - `c & 0x80 == 0`：后接 1 字节值，重复 `n` 次。
  - 解码输出长度 = `raw_len`，即解码后的原始像素字节数。
- 颜色格式：`sign==0x10` 为 256 色调色板索引图（1024 字节 ARGB 调色板 + 每像素 1 字节）；`sign==9` 为 16 色调色板索引（64 字节调色板 + 4bpp）；`sign==14` 灰度；`bpp==4` 为 ARGB8888；`bpp==2` 为 RGB565；`sign==6` 为 RGB888。
- 放大/缩放：表盘元素自身的 `x/y/w/h` 布局参数在 `wfDef.json`；工具侧不做缩放，缩放由固件/UI 完成。`mi8pro` 允许 alpha 与 2×2 对齐的 16 色图。

> 对 NES 上屏的启示：RLE V2.0 是成熟的“索引色 + 游程”编码，可作为**文件型回退**（把帧写成 `mi8pro` 图片再由路径加载），但需要固件解码器支持该容器，且每帧写文件/解码不划算；主路径仍应是内存 image descriptor。

### 6.3 game.elf 结构（原生渲染参考）

`images/lua/game.elf`：

- ELF32、little endian、`ET_REL`、`EM_ARM`、entry `0x28409`，15 个 section；`.text` 0x2F8C8、`.rodata` 0x66450、`.bss` 0x1B98。
- 无 undefined symbol；自带 `__aeabi_*` 软浮点；重定位仅 `R_ARM_ABS32`(1424)、`R_ARM_TARGET1`(1)、`R_ARM_THM_CALL`(6880)、`R_ARM_THM_JUMP24`(89)，**无 SVC**。
- 含 `bh_display_init/begin/step_rows/borrow/release/cancel` 等自研软件合成函数；`bh_display_begin` 对宽高做 `1..=0xA0`（160）与 `1..=0x70`（112）的上界检查，并校验目的缓冲区大小。说明其自绘路径使用 **≤160×112** 的像素区域（与手环 UI 的图层/放大策略一致），不是直接写 336×480 帧缓冲。
- `main.lua` 是安装壳：校验固件 `3.101.043`、build `825192154`、CRC、`game.elf` 长度 726,540；把 `game.elf` 复制到 `/data/minecraft10-073.elf` 后 `insmod`，轮询 `/dev/minecraft10_073` 状态；与我们的 Canopus 模块加载方式不同（原厂 modlib vs Canopus Supervisor），**不能照搬其地址或加载约定**。

> 该 ELF 没有外部符号导入，说明它通过固件传入的上下文/自绘缓冲工作；本任务不做逆向移植，只借鉴“小区域绘制 + 放大”的尺寸结论。证据等级：`STATIC_RECOVERED`（只读观察，未执行）。

---

## 7. 上屏路径证据盘点

### 7.1 Canopus 语义 UI（`ctx->ui`）

- ABI 2.0，一棵树最多 32 节点；IMAGE 节点需要 `primary[0] != '\0'` 与 `resource_id != 0`，UI host 最终调用 `image_set_src(object, primary)`。
- `.043` 的样式属性审计只有“圆角 + 圆角裁剪”；IMAGE_FIT 能力按 target 实测，`.043` 有 `EVID-UI-1043-IMAGE-FIT-001`。
- 关键限制：IMAGE 节点是**路径驱动**，没有“内存帧缓冲/每帧指针”入口；提交是增量语义树重建，不适合 20–30fps 全帧更新。
- 结论：可作为**静态封面/低帧率回退**，不能作为主上屏路径。证据：`STATIC_CONFIRMED`（规范）+ `DEVICE_PROVEN`（UI host 图片路径）。

### 7.2 target-bound 原生页面 + LVGL 绑定

- 原生页面：`ctx->launcher->register_app`（ABI 2.0）或原厂 `app_install`；页面描述符 `on_create(page, root, start_data)` 提供 **LVGL root 指针**。LyraPlayer 的 `native_app.rs` + `ui_backend.rs` 即此模型：`root` → `lvx_content_create` → `lvx_image_create` → `lvx_image_set_src(path)` → `lvx_object_set_size/align` → `lvx_image_set_scale`。
- 已恢复绑定（`canopus-target-private` / 043）：
  - `lvx_image_create(parent)`：内部按固件 `(parent, NULL)` 调用，避免 1 参 ABI 透传脏 R1 导致 Bus Fault（源码注释明确）。证据：STATIC_RECOVERED（封装）+ 底层 `lv_image_create` DEVICE_PROVEN。
  - `lvx_image_set_src(image, source)`：底层 `lv_image_set_src` 为 **DEVICE_PROVEN**（EVID-UI-1043-DEVICE-001），`source` 由 classifier + `lv_image_decoder_get_info` 处理，路径字符串与变量 descriptor 都走同一入口。
  - `lvx_image_set_scale(image, sx, sy)`：Q8，256=100%，`.043` 与 `.036` 结构等价；STATIC_RECOVERED/restricted。
  - `lvx_object_set_size/align/move_to_index/set_hidden`、`lvx_content_create`、`lvx_timer_*` 等；LyraPlayer 已在同 target 使用。
- **帧源格式**：固件是 LVGL v9（`lv_image_*` 命名、`lv_image_decoder_get_info`、12 字节 `lv_image_header_t`）。管理器图标 `.bin` 实测头为：
  ```
  19 10 00 00 | w u16 | h u16 | stride u16 | reserved u16 | pixels...
  magic=0x19, cf=0x10(ARGB8888), stride=w*4
  ```
  该 12 字节头 + 像素即 `lv_image_dsc_t.header`；内存 descriptor 在头后是 `data_size u32`、`data ptr`，`data` 偏移为 +16（LVGL v9.0/9.2 均为 +16）。
- NES 调色板索引 → RGB565 转换后写入 descriptor 像素区；`cf` 用 RGB565（`0x12`，原生屏幕同族），stride=w*2。证据：`STATIC_CONFIRMED`（LVGL v9 格式 + 管理器 .bin 实测）；RGB565 变量图未在设备验证，属 `BUILD_ONLY`/待设备。
- 逐帧刷新：`lv_image_set_src` 会重新取头并 invalidate widget 区域（EVID-RESOURCE-1043-001 记录其调用 `lv_image_decoder_get_info` 后更新尺寸并 invalidate）。因此同尺寸换源不需要额外 `lv_display_invalidate_area`。`lv_display_invalidate_area` 仍是 STATIC_RECOVERED/restricted，**不作为主路径依赖**。

### 7.3 原生绘制任务（不作为主路径）

- `p67_battery_image_dsc_init` + `p67_battery_image_draw(layer, descriptor, area)`：112 字节 `lv_draw_image_dsc_t`，source@+28，scale@+48/+52，pivot@+64/+68；可把 ARGB8888 缓冲提交到“当前 native draw layer”。
- 但需要“当前 layer”且状态为 `STATIC_RECOVERED` / `device: not_probed` / approval PENDING，且批注明确“不要替换 display draw-buffer 指针或编辑 framebuffer”。仅作为极端回退，不提升证据等级。

### 7.4 屏幕分辨率

- 解包 `preview.png`：336×480。
- 运行时必须 `ctx->display->get_resolution(ctx, &w, &h)` 复核；缩放/黑边按整数倍或 LVGL Q8 scale 计算。

---

## 8. 证据等级汇总

| 结论 | 证据等级 | 来源 |
|---|---|---|
| target 工具链、`canopus new/build/package` 可用 | BUILD_ONLY（实测） | 本报告 §4 |
| x-nes 可交叉编译并链接为 ET_REL | BUILD_ONLY（实测） | 本报告 §5 |
| x-nes `Emulator` 76,360 字节、帧 61,440 | HOST_TESTED（`size_of`） | §5.3 |
| resource.bin 容器/文件清单 | STATIC_RECOVERED（解包实测） | §6 |
| mi8pro RLE V2.0 格式 | STATIC_CONFIRMED（工具源码） | §6.2 |
| game.elf 结构、bh_display 160×112 上界 | STATIC_RECOVERED（只读） | §6.3 |
| UI ABI IMAGE 为路径驱动、32 节点 | STATIC_CONFIRMED（规范） | §7.1 |
| `lv_image_create` / `lv_image_set_src` 设备验证 | DEVICE_PROVEN（Canopus 维护者） | §7.2 |
| `lvx_image_set_scale`、`lv_display_invalidate_area` | STATIC_RECOVERED | §7.2 |
| 内存 descriptor（RGB565 变量图）逐帧上屏 | BUILD_ONLY / 待设备 | 本报告推断 |
| 336×480 分辨率 | STATIC_RECOVERED（PNG） | §7.4 |
| 任何“设备已能显示 NES 画面” | 未验证 | — |

---

## 9. 风险与未决项

1. **LVGL v9 内存 descriptor 的 `data` 偏移**：v9.0/v9.2 均为 +16，但固件具体小版本未取得头文件；阶段 2 用“+16 与 +20 双写指针”的保守结构，并保留路径回退。
2. **RGB565 变量图是否被固件 draw 路径直接接受**：未设备验证；若不接受，回退 ARGB8888（同尺寸内存翻倍，或缩小到 96×96）或路径 `.bin`。
3. **128K 堆是否真实**：TASKSPEC 按 128K 设计；Canopus 记录 2026-10-19 将基线扩到 256 KiB，但 `.043` 固件实际放行量未知。阶段 2 先按 128K 选 128×96，运行时用 `ctx->mem->available/used` 记录真实预算。
4. **CPU 性能未知**：Cortex-M33 主频与 x-nes 每帧耗时未测；采用“每 tick 至少一帧、按耗时自适应跳帧”的策略，显示 20fps 上限，游戏速度允许降速。
5. **原生页面注册**：`ctx->launcher` 依赖 Manager UI；若目标构建无 Manager，需回退到原厂 `app_install` 或 UI ABI 低帧率路径。阶段 2 先按 LyraPlayer 的 native page 形态实现。
6. **不得跨 target 推断地址**：所有地址仅使用 `targets/xiaomi-band-10-pro-3.101.043` 的已恢复记录；不引用 036/155 地址作为 043 证据。

---

## 10. 可复现命令清单

```bash
# 0) 基址
git clone --depth 1 https://github.com/oraserz/workspace.git /workspace/nes-band10pro/base
git -C /workspace/nes-band10pro/base pull --ff-only
git -C /workspace/nes-band10pro/base rev-parse HEAD   # 5df98ba6d1a8c5351b289e878e29c1c30f21d524

# 1) 工具链
cd /workspace/nes-band10pro/vendor/Canopus && cargo build --release -p canopus-cli
ln -sfn ../vendor/Canopus/target/release/canopus /workspace/nes-band10pro/tools/bin/canopus
cd /workspace/nes-band10pro/probe/hello && ../../tools/bin/canopus doctor

# 2) 最小工程
../../tools/bin/canopus build && ../../tools/bin/canopus package

# 3) x-nes 交叉编译与链接
cd /workspace/nes-band10pro/probe/xnes-probe
cargo +1.97.0 build --release --target thumbv8m.main-none-eabi
clang --target=thumbv8m.main-none-eabi -mcpu=cortex-m33 -mfloat-abi=soft -mthumb -O2 \
      -c link/probe_stub.c -o link/probe_stub.o
ld.lld -m armelf -r -o link/module_probe.o link/probe_stub.o \
      target/thumbv8m.main-none-eabi/release/libxnes_probe.a

# 4) resource.bin 解包
cd /workspace/nes-band10pro/vendor/Mi8WfBinTool && javac Mi8WfBinTool.java
mkdir -p /workspace/nes-band10pro/build/resource_unpacked/images/lua
java Mi8WfBinTool unpack /workspace/nes-band10pro/base/resource.bin \
     /workspace/nes-band10pro/build/resource_unpacked mi8pro
```

---

## 11. 阶段 1 完成判据对照

- `docs/01-research.md`、`docs/DECISION.md` 存在且结论明确：**是**。
- 最小工程可 `canopus build`：**是**（PASS，0 undefined）。
- x-nes 交叉编译成功：**是**（rlib + ET_REL 链接，0 undefined）。
- `resource.bin` 解包与格式分析落地：**是**（见 §6，并明确样本无 RLE 图元）。
