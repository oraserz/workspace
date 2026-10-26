# 阶段 2 实现报告：NES 帧输出到小米手环 10 Pro 屏幕

- 执行日期：2026-10-27（Asia/Shanghai）
- 基址仓库：`https://github.com/oraserz/workspace.git`，commit `5df98ba6d1a8c5351b289e878e29c1c30f21d524`（只读 `pull --ff-only`，未推送）
- 目标：`xiaomi-band-10-pro-3.101.043`（armv8-m.main / Cortex-M33 / Thumb2 / soft float）
- 阶段 2 范围：正式模块工程、x-nes 集成、原生页面、ROM 读取、128×96 RGB565 逐帧上屏、构建 + 打包 + 主机侧验证
- 实现依据：`docs/DECISION.md`（D1 target-bound、D2 原生页面 + LVGL v9 descriptor、D4 内存预算、D5 只承诺 NROM）

> 证据等级沿用 Canopus：`STATIC_CONFIRMED` > `STATIC_RECOVERED` > `DEVICE_PROVEN`（本仓库内指 Canopus 维护者真机验证过的能力）> `HOST_TESTED` > `BUILD_ONLY`。
> **本文件中的“编译通过 / 主机验证”一律不等于设备验证。当前设备未验证。**

---

## 1. 完成判据对照

| 判据（`prompts/02-implement.md`） | 状态 | 证据 |
|---|---|---|
| 模块可 `canopus build`，verifier 通过 | 达成 | `summary: PASS (14 sections, 311256 resident bytes, 0 undefined, 18700 relocs, 1 ctors, 1 dtors)` |
| `.canopus` 包存在 | 达成 | `nesband10pro/dist/nesband10pro-0.1.0.canopus`，661,504 B，versionCode 1 |
| 存在明确的“帧 → 屏幕”路径 | 达成（实现层） | 见第 5 节：`tick()` → 61,440 字节索引帧 → 就地 RGB565 → A/B `ImageDsc` → `lvx_image_set_src` |
| 测试 ROM 可被模块加载并跑帧（至少主机侧） | 达成（主机侧） | `probe/host-render`：`check=true`、`cycles/frame=29780`、`in-place conversion matches separate buffer: true` |

设备侧四项（页面可打开、descriptor 被接受、字节序、真实帧率/堆余量）**未验证**，见第 10 节。

---

## 2. 工程结构

```
nesband10pro/
├── Canopus.toml          schema=1, id=nesband10pro, kind=target, language=rust, canopus_abi=2
├── Cargo.toml            staticlib, edition 2021, rust-version 1.97.0
├── src/
│   ├── lib.rs            模块描述符、生命周期回调、GlobalAlloc、query
│   ├── nes.rs            ROM 查找/读取、Emulator 分配、step/convert、状态计数
│   ├── frame.rs          64 色调色板 RGB565 LUT + 别名安全降采样转换
│   └── ui.rs             launcher/page 描述符、原生页面生命周期、A/B descriptor、定时器
├── build/                cargo 中间产物 + 合并后的 ET_REL
└── dist/                 .canopus 包
```

依赖（`Cargo.toml`）：
- `canopus-abi`、`canopus-runtime`：绝对路径指向 `../vendor/Canopus/sdk/rust/...`
- `canopus-target-private`（`optional`、`default-features = false`）：提供 `lvx_*` 绑定与 `app_lookup/app_install/launcher_add`
- `x-nes`：`path = "../vendor/x-nes"`、`default-features = false`（no_std）

feature 只有两条：`device`、`target-xiaomi-band-10-pro-3-101-043`（后者启用前者与 target-private 的同名 feature）。

---

## 3. 生命周期与注册

Canopus 的 Rust 模块入口由 `sdk/rust/c_shim/canopus_ctor.c` 注入（`.init_array` 里调 `canopus_module_descriptor`，`.fini_array` 里调 `canopus_mod_stop(0)`）。

| 回调 | 线程/约束 | 本模块行为 |
|---|---|---|
| `canopus_mod_prepare` | stock `insmod` 在 **7.9 KiB 栈**上调用，必须无分配 | 只 `bind_context(ctx)`，返回 0 |
| `canopus_mod_activate` | 模块槽激活后 | `nes::init(ctx)`：找 ROM → 解析 → 分配 Emulator → 挂像素指针；写 `LOAD_STATE` |
| `canopus_mod_query` | 任意 | 写 9 个 u32：`MAGIC / LOAD_STATE / ready / frames / last_frame_ms / width / height / heap_used / heap_available` |
| `canopus_mod_publish_native_app` | heartbeat 旧入口 | 返回 `-103`（不走 CMR1 兼容路径） |
| `canopus_mod_publish_native_app_stage(stage)` | 前台 UI heartbeat，每轮只推进一步 | `ui::install_stage(stage)` |

`ModuleDescriptorV1.flags = HAS_NATIVE_APP | NATIVE_APP_STANDALONE | REGISTERS_LAUNCHER_ENTRY | REQUIRES_UI_DISPATCHER | APP_UNREGISTER_REBOOT_REQUIRED`；`target_id` 由 `env!("CANOPUS_TARGET_ID")` 写入。

两阶段安装（与 LyraPlayer `native_app.rs` 同构，目的是把“注册”和“加入启动器”拆到两个 heartbeat）：

1. **stage 1**：`app_lookup(0x4E45)` 已存在则直接返回 0（幂等）；否则填 `launcher_app_descriptor`（`package_name = "com.canopus.nesband10pro"`、`launcher_icon_resource = "/resource/app/launcher/compass.bin"`、`app_id = 0x4E45`）与 1 个 `firmware_page_descriptor`（`page_name = "nes_player"`、`page_kind = 2`、`layer = 4`、`lifecycle_state = 4`、`page_id = 0`），调 `app_install(app, pages, 1)`；随后再 `app_lookup` 并用 `package_matches`（读 `+8` 的 package 指针）复核，失败返回 `-100` / `-101`。
   - `app_install` 会**深拷贝** `package_name`（+8）、`launcher_icon_resource`（+12）、`owned_string_20`（+20）、`owned_string_24`（+24），所以模块侧字符串必须是有效 NUL 结尾常量。
2. **stage 2**：校验已安装实例的 package，再调 `launcher_add(0x4E45)`（返回值是启动器簿记，不是状态码）。

页面回调（`on_create / on_resume / on_pause / on_destroy`）：`on_create(page, root, start_data)` 里 `root` 是 LVGL 根对象；未就绪时创建 label 显示错误文本，就绪时建 image widget 并启动定时器。

---

## 4. ROM 来源与限制

查找顺序（每步读第一份存在的文件）：

```
1) ctx->fs->get_data_path()      → /data/canopus/m.nesband10pro
   game.nes / pattern.nes / rom.nes
2) /data/canopus/nesband10pro/{game,pattern,rom}.nes
3) /data/canopus/{game,pattern,rom}.nes
```

- `File::open(ctx, path, FS_READ)` + `size()` + 1 KiB 分块读；**文件大小上限 64 KiB**。这不是随手设的：NROM 最大 32 KiB PRG + 8 KiB CHR，而 128 KiB 模块堆装不下更大的 `Vec`，超限会在分配处 abort 而不是返回错误。
- 解析后先 `drop(bytes)`，`create_mapper()` 后再 `drop(rom)`，避免 `Rom`（prg+chr）与 mapper 结构体同时驻留。
- **只承诺 mapper 0（NROM）**。其他 mapper 返回 `-1000 - mapper_id`，`query` 可读出具体 id，不静默错绘。
- 错误码：`-2` ROM 不存在、`-22` ROM 非法、`-12` 堆不足。

---

## 5. 帧路径（“模拟 → 屏幕”）

### 5.1 模拟

`nes::step()`：
1. 清 `bus.ppu.frame_complete`；
2. 循环 `emulator.tick()` 累加周期，直到 `frame_complete` 或 `BUDGET_CYCLES = 60,000` 用尽；
3. 返回是否产出了新帧。

主机实测 NROM 测试 ROM 为 **29,780 cycles/帧**（60.1 fps 理论上限，1.789773 MHz）。50 ms 定时器下每次只跑一帧，`BUDGET_CYCLES` 只是防跑飞的保险。

### 5.2 转换（就地、别名安全）

`nes::convert()` → `frame::convert_raw(src, dst, 128, 96)`：

- `src` = `&Ppu::frame`（256×240 = 61,440 B 调色板索引帧）；
- `dst` = **同一个 PPU 帧缓冲**中按 2 字节对齐后的地址（`pad = addr & 1`）；
- 每行先在栈上 `[0u16; 128]` 组装，再整行 `copy_nonoverlapping` 写出。`sy = y*240/96` 的增长快于 `y`，因此写第 `y` 行永远不会破坏后面还要读的源行——这是“就地写回”成立的前提，已在主机上以**奇数起始地址**验证（见第 9 节）。

代价：**不再单独分配 24,576 B 显示缓冲**（D4 原方案的这一项被实际实现替换）。

### 5.3 descriptor 与刷新

`ImageDsc` 是 LVGL v9 `lv_image_dsc_t` 兼容的 24 字节布局：

```
+0  u8  magic = 0x19        +1  u8  cf = 0x12 (RGB565)     +2  u16 flags = 0
+4  u16 width  = 128        +6  u16 height = 96
+8  u16 stride = 256        +10 u16 reserved = 0
+12 u32 data_size = 24,576  +16 ptr data  = pixels (模块堆内)
+20 ptr user_data = null
```

- 4 条 `const _: () = assert!(...)` 在编译期锁住字段偏移与 `size_of == 24`。
- `DESC: [ImageDsc; 2]` 两份，每次刷新 `SOURCE_INDEX.fetch_xor(1)` 交替使用：两份 descriptor 指向同一像素区，但对象地址不同，用来给 LVGL 制造 “source changed” 的差异（**BUILD_ONLY，需设备确认真的触发重绘**）。
- 刷新序列（`set_source`）：`lvx_image_set_src(image, dsc)` → `lvx_image_set_scale(image, 512, 512)` → `lvx_object_set_size(image, 256, 192)` → `lvx_object_align(image, ALIGN_TOP_MID, 0, 172)`。
  - **整数倍缩放**：2×（Q8 512/256），`128×96 → 256×192`，不做非整数拉伸；
  - **黑边**：336 宽屏内容区左右各留 40 px，垂直在 `CONTENT_TOP=56..480` 内居中（`56 + (424-192)/2 = 172`），上下黑边 172/116 px；
  - 页面创建时调用 `ctx->display->get_resolution`（`canopus_runtime::display::get_resolution`），`scale = min(screen_w/128, (screen_h-56)/96)`；服务不可用或返回 0 时回退 336×480。
- 定时器：`lvx_timer_create(on_timer, 50, null)`；`on_timer` 依次 `step()` → `convert()` → `set_source()` → `note_frame(elapsed_ms)`。`on_pause/on_destroy` 删除定时器，`on_resume` 重建。

---

## 6. 内存核算（实测，非估算）

主机 `size_of`（`probe/sizecheck`，Rust 1.97.0 release）：

| 项 | 字节 | 说明 |
|---|---:|---|
| `Emulator` | 76,360 | 含 `Ppu` 65,880（内含 61,440 帧缓冲）、`Apu` 8,368、`Bus` 76,352、`Cpu` 8 |
| `Nrom`（mapper 实例） | 49,168 | `prg` 32,768 + `chr` 8,192 + `prg_ram` 8,192 + 字段；`Mapper` 本身只是 16 B 胖指针 |
| `svc_mem_block` 头 ×2 | 16 | Supervisor 每个分配块 8 B 头 |
| 静态 descriptor / 小状态 | 196（`.bss`）+ 35,576（`.rodata` 含调色板与字符串） | 不在模块堆内 |
| **模块堆峰值** | **125,544** | 76,360 + 49,168 + 16 |
| 预算 `MEM_BUDGET` | 131,072 | `canopus-abi` Rust 侧常量 |
| **余量** | **≈ 5,528 B** | 偏紧 |

- **峰值顺序**证明可行：读文件 `Vec`（≤24,592）→ `Rom::new` 复制 prg/chr（+24,576）→ `drop(bytes)` → `create_mapper()`（+49,168，此时 −Vec）→ `drop(rom)`（−24,576）→ 分配 `Emulator`（+76,360）。任一时刻都不超过 125,544。
- 应用启动前先检查 `mem->available() >= size_of::<Emulator>() + 4,096`，不足直接返回 `-12`，不做半初始化。
- **常量不一致（必须记录）**：`canopus-abi::MEM_BUDGET = 131072`，而 `sdk/c/canopus_services.h` 的 `CANOPUS_MEM_BUDGET = 262144u`；Supervisor 的 `available = CANOPUS_MEM_BUDGET - used` 由 C 侧决定。若真机按 256 KiB 放行，余量会宽裕得多；本模块按 128 KiB 自证可行，不依赖更大预算。
- 未采用的压缩：`no-apu` feature（省 8,368 B）、缩小 mapper 的 `prg_ram`。留作阶段 3/4 的余量手段。

---

## 7. 证据等级与未验证项

| 能力 | 等级 | 来源 |
|---|---|---|
| `lv_image_create` / `lv_image_set_src` | `DEVICE_PROVEN` | `EVID-UI-1043-DEVICE-001`（Canopus 维护者真机） |
| `lvx_image_create/set_src/set_scale`、`lvx_object_set_size/align`、`lvx_timer_*` 绑定 | `STATIC_RECOVERED` | `canopus-target-private/src/band10_pro.rs`（地址来自本 target 的恢复记录） |
| LVGL v9 12 字节 image header（magic 0x19 / w@+4 / h@+6 / stride@+8） | `STATIC_CONFIRMED` | `EVID-RESOURCE-1043-001`、`EVID-P67-BATTERY-TRANSPARENCY-001`、管理器图标实测头 `19 10 00 00 | 117 | 117 | 468 | 0` |
| `lv_image_dsc_t` 的 `data_size@+12 / data@+16 / user_data@+20` | `STATIC_RECOVERED` | 上条证据 + LVGL v9 上游结构 |
| **`LV_COLOR_FORMAT_RGB565 = 0x12`** | `BUILD_ONLY` | 仓库内无 LVGL 头文件；按 LVGL v9 上游枚举推断 |
| 在模块堆内传**变量图** descriptor 并被接受 | `BUILD_ONLY` | 管理器图标是文件内 descriptor；变量图未见设备证据 |
| `lvx_image_set_scale` 的实际语义（Q8，256 = 100%） | `STATIC_RECOVERED` | 绑定注释 + LyraPlayer 用法 |
| A/B descriptor 交替能触发重绘 | `BUILD_ONLY` | 未在设备确认；回退方案见 `DECISION.md` D6 |
| 336×480 屏幕、内容区 56/424 | `STATIC_CONFIRMED`（尺寸） | `preview.png`、`LyraPlayer player_layout.rs`；运行时已用 `ctx->display->get_resolution` 复核（服务返回 0 时回退 336×480，`BUILD_ONLY`） |

---

## 8. x-nes 本地补丁清单（`vendor/x-nes`，未上游）

`git diff --stat`：12 文件、+63/−2（`patches/x-nes-043.patch` 为完整补丁，5,889 字节）。

1. `Cargo.toml`：`crate-type = ["rlib"]`（no_std 静态库需要；上游默认配置不可直接被 `ld.lld -r` 消费）。
2. `src/mapper/common.rs`：`use alloc::vec;`；`konami/vrc1.rs`、`vrc2_4.rs`、`vrc3.rs`、`vrc6.rs`、`vrc7.rs`：`use alloc::vec::Vec;`（no_std 下缺导入）。
3. `src/ppu/mod.rs`、`src/apu/mod.rs`、`src/bus.rs`、`src/lib.rs`：各加 `pub unsafe fn init_in(ptr)`，用于**就地初始化**已分配的内存（避开 `Emulator::new` 的栈上构造，Cortex-M33 上 76 KB 结构体必然爆栈）。
4. `src/ppu/registers.rs`：修 `write_scroll` 第二次写的掩码 bug——原 `self.t & 0xFC1F` 会保留旧 fine Y，导致画面整体下移；改为 `self.t & 0x0C1F`（只保留 coarse X 与 nametable 位）。该 bug 用测试 ROM 复现（`y 0..29` 全为 `0x0F`、`y 30` 起才是棋盘格），修复后从 `y 0` 起正确。

补丁都是最小改动，未改渲染算法、未裁剪 APU 逻辑。

---

## 9. 构建产物与主机验证

### 9.1 构建 / 打包

```bash
cd /workspace/nes-band10pro/nesband10pro
RUSTUP_TOOLCHAIN=1.97.0 CARGO=/root/.cargo/bin/cargo \
  /workspace/nes-band10pro/vendor/Canopus/target/release/canopus build
# built ./build/xiaomi-band-10-pro-3.101.043/nesband10pro.elf
# sha256: 39ce46610c5db68061d8a0f4498a43f62c9decd1890b06db0503aaafc47cf97c
# summary: PASS (14 sections, 311256 resident bytes, 0 undefined, 18700 relocs, 1 ctors, 1 dtors)

RUSTUP_TOOLCHAIN=1.97.0 CARGO=/root/.cargo/bin/cargo \
  /workspace/nes-band10pro/vendor/Canopus/target/release/canopus package
# wrote ./dist/nesband10pro-0.1.0.canopus (661504 bytes, 1 target(s), versionCode 1)
# package verify -> package OK: nesband10pro v0.1.0 (1 artifact(s))
```

ELF 段（llvm-readelf）：`.text` 275,476 / `.rodata` 35,576 / `.bss` 196 / `.init_array` 4 / `.fini_array` 4。
`.rodata` 里是 64×2 B RGB565 调色板、字符串与小常量；**没有把帧缓冲放进静态 BSS**。
`0 undefined` 说明未触发 `__aeabi_mem*` 等 libc 依赖（Rust 构建路径不编译 `canopus_aeabi_mem.c`，这一点由 verifier 的零导入策略守住）。

注：`canopus build` 默认用 PATH 上的 `cargo`（本机为 1.92.0），低于 crate 的 `rust-version = 1.97.0`；必须用 `CARGO` 环境变量 + `RUSTUP_TOOLCHAIN` 指向 1.97.0。

### 9.2 测试 ROM（脚本生成，无版权内容）

`scripts/gen_test_rom.py` 用内置 6502 汇编器生成 24,592 B 的 iNES NROM：
- 复位序列 → 两次等 vblank → 关 NMI/渲染 → 写 `$3F00` 调色板 `[0x0F, 0x16, 0x2A, 0x12] + [0x0F]×28` → 填 `$2000` 起的 960 B nametable（`1 + ((col//4 + row//4) % 3)` 棋盘格）+ 64 B 属性表 → `$2006/$2005` 复位滚动 → 开 BG；
- CHR 是 4 个纯色 tile。

产物：`assets/pattern.nes` 与 `build/testroms/pattern.nes`。

### 9.3 主机侧验证（`probe/host-render`）

std 二进制，通过 `#[path = "../../../nesband10pro/src/frame.rs"]` **复用设备端同一份转换代码**：

```bash
cd /workspace/nes-band10pro && ./probe/host-render/target/release/host-render assets/pattern.nes build/host-render
# frame 1: 29780 cycles, unique colours 1 -> 1
# frame 2: 29783 cycles, unique colours 2 -> 2
# frame 8: 29781 cycles, unique colours 3 -> 3
# ppu: mask=0x0a ctrl=0x08 v=0x1001 t=0x0000 render_v=0x0000 fine_x=0
# in-place conversion matches separate buffer: true
# frames=8 snapshots=3 cycles/frame=29780 check=true
```

- `check=true`：32×32 块颜色循环严格等于 `[0x16, 0x2A, 0x12][(bx+by)%3]`，且第 0 行就是棋盘格（不是下移 30 行）。
- `in-place ... true`：把索引帧拷到 `vec![0u8; 57 + 61440]` 的 `+57`（奇数地址），`dst = src + 1`，就地转换结果与独立缓冲逐字节一致——这是第 5.2 节别名论证的实证。
- 输出 `build/host-render/frame{1,2,8}.{ppm,png,indices}`（PNG 由 `scripts/ppm_to_png.py` 生成，已目视确认）。
- 等级：**`HOST_TESTED`**，与设备无关。

### 9.4 整数倍缩放 / 黑边主机合成

按设备端 `layout()` 的同一组参数（336×480、`CONTENT_TOP=56`、2× 整数缩放）把 frame 8 合成为 336×480 预览：

```bash
# 128×96 → 256×192，居中于内容区
# origin (40, 172)，非黑像素边界 (40,172)-(295,363)，即 256×192
python3 scripts/ppm_to_png.py build/host-render/frame8-screen.ppm build/host-render/frame8-screen.png
```

- 产物：`build/host-render/frame8-screen.{ppm,png}`（336×480）。
- 目视确认：棋盘格完整、无裁切；左右各 40 px、上 172 px、下 116 px 黑边。
- 等级：**`HOST_TESTED`**（缩放参数与设备一致，但 LVGL 实际绘制仍属设备待验证）。

---

## 10. 设备侧待验证（用户执行）

1. `insmod` 后 `app_lookup(0x4E45)` / 启动器能看到 “NES” 与 compass 图标（图标是借用内置资源，非自有美术）。
2. 进入 `nes_player` 页：image widget 是否出现、是否显示棋盘格。
3. `LV_COLOR_FORMAT_RGB565 = 0x12` 是否被固件接受（若花屏/全黑，第一优先怀疑此项；回退见 D6）。
4. RGB565 字节序（LVGL 期望本机小端；若红蓝互换说明应为 BGR565 或需交换字节）。
5. `lvx_image_set_scale(512)` 是否真的按 2× 放大且不裁切；`get_resolution` 返回值是否与 336×480 一致；A/B descriptor 交替是否触发重绘（否则尝试同 descriptor + `lv_display_invalidate_area`，等级 `STATIC_RECOVERED`）。
6. 实测帧率与 `query[4]`（`last_frame_ms`）；`query[7]/[8]`（heap_used/available）验证第 6 节核算。
7. ROM 放置：把 `pattern.nes` 推到 `/data/canopus/m.nesband10pro/pattern.nes`（或模块 `get_data_path` 对应目录），重启模块后打开页面。

---

## 11. 边界与未做

- 未实现音频输出、按键输入、存档；屏幕尺寸运行时取自 `ctx->display->get_resolution`，返回 0 时回退 336×480。
- 未做跳帧策略：每 50 ms 固定跑一帧，慢于实时即自动落后，不做追帧。
- 只支持 mapper 0；其他 mapper 明确报错不静默。
- 未向基址仓库推送任何内容；未提交任何版权 ROM 或固件 `bin`（`assets/pattern.nes` 由脚本生成，`resource.bin` 仅本地解包分析）。
