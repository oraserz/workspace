# 决策记录（DECISION）

日期：2026-10-27
target：`xiaomi-band-10-pro-3.101.043`（只针对该 target，不做跨 target 推断）

## D1 模块形态：target-bound Rust staticlib

- 选择 `canopus-target-private` + `canopus-target-generated`（feature `target-xiaomi-band-10-pro-3-101-043`），`crate-type = ["staticlib"]`。
- 理由：Canopus UI ABI 2.0 的 IMAGE 节点只接受固件可读路径，无法承载逐帧内存画面；上屏必须直接使用 target 的 LVGL 包装层。portable 路径（canopus-ui-core）无法满足本轮“把画面输出到手环屏幕”。
- 代价：模块只能在 043 上加载；`Canopus.toml` 的 `[targets].include` 只列 043。
- 证据：`canopus build` + ELF verifier PASS（见 REPORT）。

## D2 上屏路径：target-bound LVGL 图像对象 + 定时刷新

- 页面根位于原生应用页面回调 `on_create(root)`；在 `on_create` 内：
  1. `lvx_content_create(root)`，尺寸取运行时面板分辨率（`ctx->display->get_resolution`），`ALIGN_TOP_MID`。
  2. `lvx_image_create(content)`，`lvx_object_set_size(256, 240)`，`lvx_object_align(ALIGN_TOP_MID, 0, (h-240)/2)`。
  3. `lvx_image_set_src(image, dsc)`，`dsc` 是模块静态的 LVGL v9 `lv_image_dsc_t`（`magic=0x19`、`cf=RGB565(0x12)`、`256×240`、`stride=512`、`data=RGB565 缓冲`）。
  4. `lvx_timer_create(frame_timer, 100ms, null)`，回调在 UI 线程里跑一帧 x-nes 并改写描述符后再次 `lvx_image_set_src`。
- 逐帧失效：`lv_display_invalidate_area` 在 private 层无 wrapper，改由两个描述符交替（`DESCRIPTORS[0/1]`）保证源指针变化，绕开“相同指针短路”。
- 证据边界：`lv_image_create` / `lv_image_set_src` 的文件路径用法是 DEVICE_PROVEN（LyraPlayer），**本模块传入内存描述符是新增用法，只有静态/主机证据，设备未验证**。

## D3 帧率与耗时

- 定时器周期 100 ms（目标 ~10 fps），与 LyraPlayer 在本 target 的刷新周期一致；每帧指令预算上限 `4_000_000`，超出则本帧跳过，避免卡死 UI 线程。
- 主机 release（x86_64）实测 **2.7 ms/帧**（60 帧测试 ROM）；M33 上未测，因此没有把帧率写成 20–30 fps，也没有声称达到该区间。
- 设备侧待验证：单帧耗时、UI 线程占用、是否需要跳帧策略。

## D4 ROM 来源与内存

- 不引入任何版权 ROM。测试 ROM 由 `tools/make_test_rom.py` 用 6502 手写生成（NROM / mapper 0，16 KiB PRG + 8 KiB CHR），图案为自绘色带 + 逐帧滚动；输出 `nes_band/assets/test_band.nes`（24592 字节）。
- 模块内 ROM 通过 `include_bytes!` 进 `.rodata`；运行时仅支持 NROM（mapper 0），其余 mapper 回退行为未验证，属于已知限制。
- 内存核算（主机实测/结构体尺寸）：
  - `Emulator` = **76360 字节**（Ppu 的 61440 字节 frame 为主）。
  - 本地补丁后的 `Nrom`：PRG 32 KiB + CHR 8 KiB + PRG RAM 8 KiB = **48 KiB**（堆上）。
  - RGB565 帧缓冲 256×240×2 = **122880 字节**。
  - 合计约 **244 KiB**，超出模块动态堆 `MEM_BUDGET = 131072`（128 KiB）。
- 因此使用 `FirmwareAllocator`（`bt_alloc`/`bt_free`，头部存原始指针 + 对齐）作为 `#[global_allocator]`，复用 LyraPlayer 已验证的模式。
- **风险**：`bt_alloc` 在 target symbols 中仍是 restricted / approval PENDING。本轮只做复用，不提升其证据等级；设备侧必须确认分配是否获批、池是否够大。

## D5 x-nes 本地补丁（`lib/x-nes`）

1. `crate-type = ["lib"]`：staticlib/cdylib 在 no_std target 上会要求本库自带 allocator 与 panic handler，与模块冲突。
2. 新增 `init_in_place`（`Emulator`/`Bus`/`Ppu`/`Apu`）：避免在 7.9 KiB insmod 栈上构造 60 KiB 的 `Ppu` 临时值。
3. `Nrom` 的 `prg/chr/prg_ram` 固定数组改为 `Vec`：把 48 KiB 从栈/静态区移到堆。
4. 其余 mapper 文件补 `use alloc::vec::Vec;` / `common.rs` 补 `vec` 宏，保证 no_std 下可编译。

以上补丁只影响内存布局与初始化方式，不改模拟语义；主机与设备共用同一份 `lib/x-nes`。

## D6 应用注册

- 采用两阶段发布：`canopus_mod_publish_native_app_stage(ctx, 1)` 调 `app_install` + `app_lookup` 校验 package；stage 2 调 `launcher_add`。单次 `canopus_mod_publish_native_app` 返回 `-103`（不支持）。
- 描述符按 043 布局填写（`launcher_app_descriptor` 64 字节 / `firmware_page_descriptor` 112 字节，页面回调用 `on_destroy`）。
- 生命周期 `resident-after-activation`：deactivate 返回 `RESULT_REBOOT_REQUIRED`。
