# 02 — 实现与产物

## 1. 工程结构

```
/workspace/nes-band10pro/
├── nespro/                     # Canopus target-bound Rust 模块（交付主体）
│   ├── Canopus.toml            # module id/version/kind=target, targets = 043
│   ├── Cargo.toml              # staticlib；依赖 canopus-abi/runtime/target-private/target-generated + x-nes
│   ├── build/xiaomi-band-10-pro-3.101.043/nespro.elf
│   ├── dist/nespro-0.1.0.canopus
│   └── src/
│       ├── lib.rs              # 生命周期、launcher 注册、LVGL 页面、定时器、分配器
│       └── nes_frame.rs        # 调色板 → RGB565 转换（设备与主机共用）
├── roms/nespro-test.nes        # 自产 16 KiB NROM 测试 ROM（include_bytes! 进模块）
├── tools/
│   ├── make_test_rom.py        # 生成上述测试 ROM
│   └── ppm_to_png.py           # 主机侧 PPM → PNG
├── hostverify/                 # 主机侧验证二进制（同一 x-nes + nes_frame.rs）
│   └── out/*.ppm / *.png       # 主机验证证据
├── docs/                       # 01-research / DECISION / 02-implementation
├── analysis/mi8/out/           # resource.bin 解包产物（仅分析证据，不发布）
└── vendor/canopus, vendor/x-nes
```

## 2. 模块行为

**模块描述符**（`canopus_module_descriptor`，ABI 2）：

- `module_id = "nespro"`、`version = "0.1.0"`、`target_id = env!("CANOPUS_TARGET_ID")`
- flags：`FLAG_HAS_NATIVE_APP | FLAG_REGISTERS_LAUNCHER_ENTRY | FLAG_APP_UNREGISTER_REBOOT_REQUIRED`
- 生命周期：`prepare` 保存 ctx；`activate` 记录日志；`stop` 释放标记；`query` 返回 `0x4E455350`（"NESP"）

**launcher 注册**（stage 1）：创建 116 字节 `firmware_page_descriptor`（字段偏移与 `targets/.../types/firmware_page_descriptor.json` 完全一致，编译期 `offset_of` 断言校验 `on_create=0x4c`、`on_destroy=0x5c`、`size=0x74`），填入：

| 字段 | 值 |
| --- | --- |
| page_name | `"nes_main"` |
| page_id | 0（与 native-app-demo 一致） |
| on_signal / on_create / on_resume / on_pause / on_destroy | 模块函数 |

然后 `ctx->launcher->register_app(ctx, AppEntryV1{ label:"NES", icon:"/resource/app/launcher/compass.bin", pages, page_count:1 })`。stage 2 执行 `activity_navigate(app_id << 16, 0, 0, 0)` 进入页面。

**页面创建**（`page_on_create(page, root, start_data)`）：

1. `lvx_content_create(root)` → `lvx_object_set_size(336,424)` → `lvx_object_align(TOP_MID,0,56)`。
2. `lvx_image_create(content)` → `init_dsc()` → `lvx_image_set_src(image, dsc)` → `lvx_image_set_scale(image,336,336)` → `lvx_object_align(TOP_MID,0,0)`。
3. `ctx->display->get_resolution()` 校验并记录日志。
4. 读 ROM：`/data/canopus/nespro.nes` 存在则读入，否则用内嵌 16 KiB 测试 ROM。
5. `start_emulator`：`Rom::new` → `create_mapper` → `drop(rom)` → `Emulator::new` 写入 `static mut EMU`。
6. 立即跑一帧并上屏（避免空白页），再 `lvx_timer_create(frame_timer, 33ms, NULL)`。

**逐帧刷新**（`frame_timer`，UI 线程）：

- 最多执行 `120_000` 条指令或直到 `ppu.frame_complete`，然后清除标志；
- `convert_frame(&ppu.frame, &mut FB)`：256×240 调色板索引 → RGB565（`PALETTE_RGB` 与 x-nes `examples/window.rs` 同表）；
- 切换 descriptor 后 `lvx_image_set_src`，让 LVGL 重绘。

**销毁**：`page_on_destroy` 删除定时器、清空 image 指针、`EMU_READY = false`（`Emulator` 保留在原静态存储，避免析构顺序问题；模块卸载时随之失效）。

**全局分配器**：`ModuleAlloc` 实现 `GlobalAlloc`，底层 `canopus_runtime::mem`（ctx->mem，约 128 KiB）；对齐 > 8 的请求前置 4 字节保存原始指针。x-nes 的 `Vec`/`Box` 与 fs 读取都走这里。

## 3. 构建与打包（可一键复现）

前提：`CANOPUS_ROOT=/workspace/nes-band10pro/vendor/canopus`，工具链见 `01-research.md` §2。

```bash
# 1) 生成自产测试 ROM（已提交到 roms/，可重复生成）
python3 /workspace/nes-band10pro/tools/make_test_rom.py \
  /workspace/nes-band10pro/roms/nespro-test.nes

# 2) 构建模块（cargo + 链接 + ELF verifier）
cd /workspace/nes-band10pro/nespro
CANOPUS_ROOT=/workspace/nes-band10pro/vendor/canopus canopus build

# 3) 打包并校验
CANOPUS_ROOT=/workspace/nes-band10pro/vendor/canopus canopus package --build
CANOPUS_ROOT=/workspace/nes-band10pro/vendor/canopus \
  canopus package verify dist/nespro-0.1.0.canopus

# 4) 主机侧画面验证（可选，重新生成 out/*.ppm/*.png）
cd /workspace/nes-band10pro/hostverify
cargo run --release -- ../roms/nespro-test.nes out
```

## 4. 产物

| 产物 | 绝对路径 | sha256 | 说明 |
| --- | --- | --- | --- |
| 模块 ELF | `/workspace/nes-band10pro/nespro/build/xiaomi-band-10-pro-3.101.043/nespro.elf` | `289cbec389c1412d7e918bd8f35ca943ba2ea5594f3e3a07546c8d5d0cf8bb57` | verifier PASS：15 sections、515,869 resident bytes、0 undefined、19,919 relocs、1 ctor / 1 dtor |
| 安装包 | `/workspace/nes-band10pro/nespro/dist/nespro-0.1.0.canopus` | `192991909062054b10457e460dd121d57ee1278d221243affd4e2e5186515b4f` | 685,056 B；`package verify` OK；1 target |
| 测试 ROM | `/workspace/nes-band10pro/roms/nespro-test.nes` | 生成脚本可复现 | 16,400 B（16 B header + 16 KiB PRG，CHR RAM） |

### `.canopus` 包结构（tar）

```
artifacts/xiaomi-band-10-pro-3.101.043/module.elf   (680,616 B, sha256 = ELF 的 sha256)
manifest.json                                       (module_id nespro, version 0.1.0, abi 2, target_pack_revision 8)
receipts/xiaomi-band-10-pro-3.101.043.cmi           (CMI2 receipt, 176 B)
```

`manifest.json` 中 `firmware_sha256 = 5193076...56ec` 与 target.toml 一致；`reproducibility.build_environment = "canopus 0.1.0, sdk 4928f1eb..."`；`reboot_required = true`。receipt 是**未签名的 CMI2 摘要**，安装时的签名/授权流程不在本轮范围。

## 5. 设备安装（本轮未执行）

按 target loader：`insmod <path> nespro`（模块名需匹配 `^[a-zA-Z0-9_]+$`）。包内 ELF 需由设备侧 Manager/安装器落到模块目录；具体安装路径与授权流程**设备未验证**。

ROM 放置：`/data/canopus/nespro.nes`（不存在时自动使用内嵌测试 ROM）。

## 6. 已知限制

1. **未设备验证**：页面注册、LVGL 调用、缩放与刷新仅基于 STATIC_RECOVERED 入口，未在真机运行。
2. **ROM 体积**：模块堆约 128 KiB，`Emulator` 固定 76,360 B + mapper 自持 PRG/CHR。安全上限约 24 KiB ROM 数据；更大 ROM 会分配失败。
3. **单帧缓冲**：两个 descriptor 指向同一 `FB`，仅用于触发重绘，不是真双缓冲；画面撕裂风险未评估。
4. **缩放**：`lv_image_set_scale(336,336)` 由固件 LVGL 完成，主机侧只按最近邻近似预览，不代表设备像素结果。
5. **音频/按键/存档**：本轮明确不做。
6. **mapper 覆盖**：x-nes 全 mapper 已链接，但内存/性能只对 NROM 测试 ROM 验证过。
