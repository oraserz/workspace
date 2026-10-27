# 阶段 2 实现记录

## 工程结构

```
/workspace/nes-band10pro/
  nes_band/                 Canopus 模块（target-bound Rust staticlib）
    Canopus.toml            模块清单（只 include 043）
    Cargo.toml              crate-type = ["staticlib"]，device feature
    assets/test_band.nes    自生成 NROM 测试 ROM（24592 B）
    assets/appicon_nes.bin  48×48 LVGL v9 BIN 图标（9228 B）
    src/lib.rs              ABI 生命周期 + 描述符 + status query
    src/machine.rs          x-nes 所有权、ROM 解析、单帧步进
    src/video.rs            调色板、RGB565 转换、居中适配（设备/主机共用）
    src/ui.rs               LVGL 页面：content + image + 100 ms 定时器
    src/app.rs              原生应用/页面描述符与两阶段注册
    src/allocator.rs        FirmwareAllocator（bt_alloc/bt_free）
  lib/x-nes/                本地补丁版 x-nes（设备/主机共用）
  host/                     主机导出器（复用 nes_band/src/video.rs）
  tools/make_test_rom.py    生成测试 ROM
  tools/make_app_icon.py    生成应用图标
  out/                      主机导出的 PPM/PNG
```

## 生命周期

- `canopus_mod_prepare`：只清状态，零分配（insmod 栈 7.9 KiB）。
- `canopus_mod_activate(ctx)`：
  1. `machine::init()`：手写解析 iNES 头 → `Mapper::from_ines`；`alloc` 出 `Emulator` 与 RGB565 缓冲；`Emulator::init_in_place` 就地初始化并 `reset()`。
  2. 成功后 `ui::configure(screen_size(ctx))`：用 `ctx->display->get_resolution` 取得实时分辨率，失败回退 336×480；据此计算图像顶部偏移 `(h-240)/2`。
- `canopus_mod_deactivate`：resident 状态返回 `RESULT_REBOOT_REQUIRED`。
- `canopus_mod_query`：MAGIC("NES1") / ACTIVE / RESIDENT / LAST_ERROR / 帧数 / 指令数 / 应用状态。
- `canopus_mod_publish_native_app` 返回 `-103`；`..._stage(1|2)` 分别注册应用与发布启动器入口。

## 画面链路

```
x-nes Ppu.frame[61440]（256×240 调色板索引）
  → video::convert_nes_frame（64 色调色板 → RGB565，122880 B）
  → ui::frame_timer（每 100 ms）
  → 交替的 lv_image_dsc_t（256×240 / cf=0x12 / stride=512）
  → lvx_image_set_src(image, dsc)
  → LVGL 在 336×480 面板上居中绘制（黑边，偏移 (40,120)）
```

- 页面销毁时先 `lvx_timer_delete`，再删除 content 根（子对象随之释放）。
- `page_pause` 只把 `VISIBLE=false`，定时器仍在但回调直接返回，避免销毁/重建对象。

## 原生应用

- `APP_ID = 0x00D1`，package `com.canopus.nes-band`，启动器图标 `/data/canopus/appicon_nes.bin`。
- `launcher_app_descriptor`（64 B）与 `firmware_page_descriptor`（112 B）按 043 布局填充；页面回调 `on_create/on_resume/on_pause/on_destroy`（043 用 `on_destroy`）。
- stage 1 `app_install` 后用 `app_lookup` 校验 package；stage 2 `launcher_add`。
- 图标资源由 `Canopus.toml` 的 `[resources]` 打进包内 `resources/common/appicon_nes.bin`。

## 内存

| 项 | 字节 |
| --- | --- |
| `Emulator` | 76360 |
| NROM（PRG 32 KiB + CHR 8 KiB + PRG RAM 8 KiB） | 49152 |
| RGB565 帧缓冲 | 122880 |
| 合计 | ≈ 244 KiB |

模块动态堆 `MEM_BUDGET` 只有 131072 字节，因此所有大块分配走 `bt_alloc/bt_free`（`FirmwareAllocator`）。`bt_alloc` 在当前 target symbols 中为 restricted / approval PENDING，本轮未提升证据等级。

## 构建与打包

```bash
cd /workspace/nes-band10pro/nes_band
CANOPUS_ROOT=/workspace/nes-band10pro/vendor/Canopus \
  /workspace/nes-band10pro/vendor/Canopus/target/release/canopus package --build

CANOPUS_ROOT=/workspace/nes-band10pro/vendor/Canopus \
  /workspace/nes-band10pro/vendor/Canopus/target/release/canopus package verify \
  dist/nes_band-0.1.0.canopus
```

结果：ELF verifier PASS（0 undefined、18655 relocs、1 ctor / 1 dtor、334232 resident bytes），包校验 OK。

## 主机导出

```bash
cd /workspace/nes-band10pro
cargo run --release --manifest-path host/Cargo.toml
```

主机通过 `#[path = "../../nes_band/src/video.rs"]` 直接编译设备同一份转换代码，输出 256×240 与 336×480 的 PPM/PNG。
