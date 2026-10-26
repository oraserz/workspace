# DECISION：小米手环 10 Pro（043）NES 上屏方案

- 依据：`docs/01-research.md`、`base/TASKSPEC.md`、`base/prompts/01-research.md`
- 基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`
- 决策日期：2026-10-27

---

## D1. 模块形态：target-bound（Rust + `canopus-target-private`）

**选择：target-bound Rust 模块，静态库 + C ctor shim，走 Canopus `xiaomi-band-10-pro-3.101.043` 的 `nuttx-modlib-elf32-rel` 模块 ABI。**

理由：

1. 逐帧上屏需要 `lvx_image_create` / `lvx_image_set_src` / `lvx_image_set_scale` 与一个原生 LVGL 父对象；portable 的 `canopus-runtime` / `canopus-ui-core` 不暴露固件指针，语义 UI IMAGE 节点只接受路径。
2. target-bound 已被同 target 的 LyraPlayer 验证为可构建、可注册原生页面、可调用 `lvx_image_*` 的形态（`native_app.rs` + `ui_backend.rs`）。
3. `canopus new --lang rust --target xiaomi-band-10-pro-3.101.043` 脚手架与 `canopus build` verifier 已支持 stock modlib 目标。
4. 代价：绑定 043 的恢复符号，必须遵守 `restricted` 策略与证据等级，不做跨 target 地址推断。

**不选择 portable**：没有原生父对象，无法挂载逐帧 image widget；UI ABI 的 32 节点语义树重建也不适合 20–30fps。

工程落点（阶段 2）：`/workspace/nes-band10pro/nes-band10pro/`（正式模块），探针保留在 `probe/` 作为证据。

---

## D2. 上屏路径：原生页面 root + LVGL image widget + 模块内存中的 LVGL v9 descriptor

**主路径（按优先级）：**

1. 模块注册原生页面（`ctx->launcher->register_app`，或原厂 `app_install` 路径），在 `on_create(page, root, start_data)` 取得 LVGL `root`。
2. `content = lvx_content_create(root)`（若页面已有内容容器则复用）。
3. `image = lvx_image_create(content)`。
4. `lvx_object_set_size(image, dst_w, dst_h)`；`lvx_object_align(image, ALIGN_TOP_MID, 0, y)`；必要时 `lvx_object_move_to_index` 置底。
5. `lvx_image_set_src(image, &descriptor)`：descriptor 是**模块自有内存**中的 LVGL v9 `lv_image_dsc_t` 兼容结构。
6. `lvx_image_set_scale(image, 512, 512)`：Q8，256=100%，512=2×；目标 128×96 → 256×192。
7. 每帧更新 descriptor 的 `data_size` 与像素缓冲；通过 **A/B 两个 descriptor 交替指向同一像素缓冲** 触发重绘（避免 `lv_image_set_src` 对同一指针短路，也避免缓存旧图）。

**descriptor 布局（保守兼容 LVGL v9.0/9.2）：**

```
+0   u8  magic      = 0x19        // LV_IMAGE_HEADER_MAGIC
+1   u8  cf         = 0x12        // LV_COLOR_FORMAT_RGB565
+2   u16 flags      = 0
+4   u16 width      = 128
+6   u16 height     = 96
+8   u16 stride     = 256         // width * 2
+10  u16 reserved   = 0
+12  u32 data_size  = 24576
+16  const u8 *data = pixels      // v9.0/v9.2 均为 +16
+20  void *user_data = NULL        // v9.2 字段，v9.0 不读
+24  u32 reserved2  = 0
```

像素：`u16` 小端 RGB565，长度 `128*96*2 = 24,576` 字节。

**为什么不用 UI ABI IMAGE 节点作为主路径：**

- 节点只接受 `resource_id + 路径字符串`，由 host 调 `lv_image_set_src(image, path)`；没有内存帧缓冲入口。
- 每帧要重建/提交 32 节点语义树，且按路径解码并缓存，无法满足 20–30fps 与“每帧不同像素”。
- 仅在原生页面不可用时作为低速回退（D6）。

**为什么不用 `lv_display_invalidate_area`：**

- 状态为 `STATIC_RECOVERED` / `restricted`；`lv_image_set_src` 自身会更新尺寸并 invalidate image widget（EVID-RESOURCE-1043-001），无需额外依赖。
- 主路径不调用 `lv_display_invalidate_area`，避免提升证据等级。

**为什么不用 `p67_battery_image_draw`：**

- 需要“当前 native draw layer”，且 `STATIC_RECOVERED` / `device: not_probed` / approval PENDING；批注明确禁止替换 display draw-buffer 或直接写 framebuffer。仅作为最后回退，不提升状态。

**证据等级：**
- `lv_image_create` / `lv_image_set_src`：DEVICE_PROVEN（Canopus 维护者在 043 真机 UI host 验证）。
- `lvx_image_set_scale`：STATIC_RECOVERED / restricted。
- 内存 descriptor + RGB565：按 LVGL v9 格式与管理器 `.bin` 实测推断，**BUILD_ONLY，待设备验证**。

---

## D3. 帧率与节奏：显示 20fps 上限，模拟自适应 1–3 帧/ tick

- 显示定时器：`ctx->time->timer_create(50ms, periodic)`（或 LVGL timer 50ms）；每次 tick 最多刷新一次屏幕。
- 每个 tick：
  1. 记录 `t0 = monotonic_ms()`；
  2. 至少跑完 1 个 NES 帧（`tick()` 直到 `ppu.frame_complete`，清标志）；
  3. 若 `elapsed < 40ms`，继续跑下一帧并覆盖，最多 3 帧（对应 60Hz 的 3 帧）；
  4. 用最后一帧做 256×240 → 128×96 转换并上屏。
- 效果：CPU 允许时游戏以 60Hz 逻辑速度运行、视觉 20fps（跳帧）；CPU 不足时自动降为慢速，但画面持续刷新。
- 记录每 tick 的模拟耗时、转换耗时、`ctx->mem->used()`，写入模块状态/日志，供阶段 3 报告引用。
- 目标：稳定 ≥20fps 视觉刷新；不以“达到 60fps”为验收条件。

---

## D4. 内存预算（按 TASKSPEC 的 ~128K 模块动态堆）

> **实现修订（2026-10-27，阶段 2）**：显示缓冲不再单独分配。`frame::convert_raw`
> 是别名安全的行式转换，RGB565 直接写回 `Ppu::frame` 内部（从帧缓冲起点按 2 字节
> 对齐后的地址开始），因此下表“显示缓冲”一项为 0，`Nrom` 结构体实测 49,168 B。
> 实测堆峰值 76,360 + 49,168 + 16 ≈ **125,544 B**，余量约 5.5 KiB。详见
> `docs/02-implementation.md` 第 6 节。

| 项 | 字节 | 说明 |
|---|---:|---|
| `Emulator`（含 Ppu/Apu/Cpu/Bus/RAM） | 76,360 | host `size_of` 实测；PPU 65,880（含 61,440 帧缓冲）、APU 8,368 |
| NROM ROM 存储（mapper 内数组） | 49,168 | `Nrom` 实测：32KiB PRG + 8KiB CHR + 8KiB PRG-RAM + 字段；`Rom` 在 `create_mapper()` 后立即释放 |
| 显示缓冲 RGB565 128×96 | 0 | 就地写入 `Ppu::frame`，不单独分配 |
| 分配块头（2 块 × 8B） | 16 | Supervisor `svc_mem_block` |
| 2× descriptor + 调色板 LUT | 256 | 2×28B + 64×2B RGB565 LUT（在 `.rodata`/`.bss`，不占堆） |
| **合计（堆）** | **125,544** | 余量约 5,528B（128K=131,072） |

- **峰值顺序**：读 ROM `Vec`（≤24,592）→ `Rom::new`（+24,576）→ drop Vec → `create_mapper()`（+49,168）→ drop `Rom`（−24,576）→ 分配 `Emulator`（+76,360）。任一时刻不超过 125,544。
- **ROM 文件上限 64 KiB**：NROM 最大 32K PRG + 8K CHR；更大的文件会在 `Vec` 分配处 abort，因此在读之前就拒绝。
- **运行时复核**：启动后读 `ctx->mem->available()` / `used()`；若可用堆 < `size_of::<Emulator>() + 4KiB`，直接返回 `-12`，不半初始化。
- **可选的进一步压缩**（仅在实测不足时）：给 x-nes 加 `no-apu` feature 去掉 8,368B APU；或缩小 `prg_ram`。不默认做，避免扩大补丁面。
- **不采用**：192KiB 静态探针堆、`bt_alloc/bt_free`（蓝牙堆，STATIC_RECOVERED 且可能影响蓝牙）、把 256×240 全帧放堆上二次拷贝。

内存来源：target-bound 模块用 `ctx->mem` 服务实现 `GlobalAlloc`（`alloc = ctx->mem->alloc(ctx, size)`，`free = ctx->mem->free(ctx, ptr)`），与 `mem` 服务的“模块动态堆”语义一致；不在静态 BSS 里放大数组。

---

## D5. ROM 策略：只承诺 NROM（mapper 0），脚本生成测试 ROM

- **首要 ROM**：`/data/canopus/nes/test.nes`（或 `ctx->fs->get_data_path` 返回的模块数据目录 + `/test.nes`），由模块 `ctx->fs` 读取。
- **生成器**：`scripts/gen_test_rom.py`（阶段 2 新建）生成最小 NROM：
  - iNES 头：`NES\x1A`，1×16KiB PRG，1×8KiB CHR，mapper 0，vertical mirroring；
  - PRG：标准复位序列、调色板、nametable 填充、滚动寄存器清零、开启 BG；
  - CHR：程序生成的色块/棋盘格 tile；
  - 画面：纯色条 + 棋盘格 + 一个缓慢滚动的色块（验证帧更新）。
  - 产物：`build/testroms/pattern.nes`（**不提交任何版权 ROM**）。
- **mapper 范围**：阶段 2 只支持 mapper 0；其他 mapper 在内存/性能预算内再逐个开启，并单独记录证据。
- **主机侧**：同一生成 ROM 用 host 构建跑 x-nes，导出 PPM/PNG，作为阶段 3 的画面正确性证据。
- 若未来使用第三方测试 ROM（nestest / 240p Test Suite），必须先确认许可证并在文档记录来源；默认不做。

---

## D6. 回退方案（按触发条件）

| 触发条件 | 回退 | 代价/证据 |
|---|---|---|
| 原生页面不可用（无 `ctx->launcher` / 无 Manager UI） | 用原厂 `app_install` 注册页面；仍不行则用 `ctx->ui` 的 IMAGE 节点 + 路径 `.bin` | 5fps 左右，路径解码；UI ABI 为 STATIC_CONFIRMED/DEVICE_PROVEN |
| 内存 descriptor（变量图）被固件拒绝或崩溃 | 同 widget 改用**路径字符串**：每帧写 `lv_image_set_src(image, "/data/canopus/nes/f%d.bin")`，4 个文件轮转（ARGB8888 或 RGB565 的 12 字节头 + 像素） | 128×96 ARGB8888=49,152B/帧，内存总量≈150KB，超 128K；需降到 96×72 或改用 RGB565 路径 |
| RGB565 变量图不被接受 | 改 ARGB8888 变量图，显示缓冲降到 96×96（36,864B）或 96×72（27,648B） | 总量≈128–137KB，需实测 |
| 堆 < 120KiB | 显示缓冲降到 96×72 RGB565；必要时给 x-nes 加 `no-apu` | 画面更小/无音频（本轮本就无音频输出） |
| 模拟性能不足 20fps | 每 tick 只跑 1 帧、显示 15fps；或把转换改为查表 + 行指针 | 游戏降速，但画面持续更新 |

所有回退都必须在 `docs/02-implementation.md` 标注证据等级，不得把“编译通过”写成“设备验证”。

---

## D7. 验收口径（本轮）

1. `canopus build` + ELF verifier 通过，`canopus package` 产出 `.canopus` 包。
2. 存在一条明确的“NES 帧 → 128×96 RGB565 → LVGL image descriptor → 屏幕”路径，代码可读、可解释。
3. 主机侧能用同一转换路径把测试 ROM 帧导出为 PNG/PPM，证明画面正确。
4. 明确声明：**设备未验证**；设备侧待验证项（原生页面是否可打开、descriptor 是否被接受、颜色字节序、实际帧率、堆余量）列在 `REPORT.md`/`STATE.md`。
5. 不提交版权 ROM、固件 bin；不向基址仓库推送。

---

## D8. 未决问题（阶段 2 首日验证）

- LVGL v9 小版本与 `lv_image_dsc_t` 的实际字段（`data` +16 已覆盖 v9.0/9.2，仍需设备确认）。
- `LV_COLOR_FORMAT_RGB565 = 0x12` 是否与固件枚举一致；屏幕原生字节序。
- `.043` 原生页面回调签名与 `firmware_page_descriptor` 字段（以 `canopus-target-private` 的 043 生成为准，不跨 target）。
- `ctx->mem` 在 target-bound stock modlib 模块中的可用量与生命周期（`activate` 后是否持续有效）。
