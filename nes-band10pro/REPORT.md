# REPORT：小米手环 10 Pro（043）NES 模拟器画面输出

日期：2026-10-27
target：`xiaomi-band-10-pro-3.101.043`（固件 043）
核心：`x-nes`（本地补丁版，`lib/x-nes`）
范围：本轮只交付“把 NES 画面输出到手环屏幕”的构建产物与主机侧画面证据；音频、按键、存档不在范围内。

## 1. 一键复现构建

前置：`rustc 1.97.0` + target `thumbv8m.main-none-eabi`（由仓库根 `rust-toolchain.toml` 固定），以及已构建的 `vendor/Canopus/target/release/canopus`。

```bash
cd /workspace/nes-band10pro/nes_band
export CANOPUS_ROOT=/workspace/nes-band10pro/vendor/Canopus
/workspace/nes-band10pro/vendor/Canopus/target/release/canopus package --build
/workspace/nes-band10pro/vendor/Canopus/target/release/canopus package verify \
  dist/nes_band-0.1.0.canopus
```

主机侧画面复现：

```bash
cd /workspace/nes-band10pro
cargo run --release --manifest-path host/Cargo.toml
```

`host` 通过 `#[path = "../../nes_band/src/video.rs"]` 编译设备**同一份**调色板/RGB565/居中转换代码；测试 ROM 为 `nes_band/assets/test_band.nes`。

## 2. 产物（绝对路径）

| 产物 | 路径 | 大小 | sha256 |
| --- | --- | --- | --- |
| 模块 ELF | `/workspace/nes-band10pro/nes_band/build/xiaomi-band-10-pro-3.101.043/nes_band.elf` | 676152 B | `870e28c9f29cbd233961b6a8f02d0a5264d521c303d95a718d3baf8afa2bd2e9` |
| 可安装包 | `/workspace/nes-band10pro/nes_band/dist/nes_band-0.1.0.canopus` | 691200 B | `ea51a7fc06cd00d5760b97229f6b54fda6cd698af44bb0bec08daea6f191ec1b` |
| 测试 ROM | `/workspace/nes-band10pro/nes_band/assets/test_band.nes` | 24592 B | 自生成（`tools/make_test_rom.py`） |
| 应用图标 | `/workspace/nes-band10pro/nes_band/assets/appicon_nes.bin` | 9228 B | 自生成（`tools/make_app_icon.py`） |
| 256×240 PPM | `/workspace/nes-band10pro/out/frame_0010_256x240.ppm` 等 3 个 | 各 184335 B | — |
| 336×480 PPM | `/workspace/nes-band10pro/out/frame_0010_336x480.ppm` 等 3 个 | 各 483855 B | — |
| 336×480 PNG | `/workspace/nes-band10pro/out/frame_0010_336x480.png` 等 3 个 | 各 484423 B | — |

## 3. 验证结果

### 3.1 编译 / ELF verifier（编译通过）

`canopus build`（经 `package --build`）：

```
built ./build/xiaomi-band-10-pro-3.101.043/nes_band.elf
sha256: 870e28c9f29cbd233961b6a8f02d0a5264d521c303d95a718d3baf8afa2bd2e9
summary: PASS (15 sections, 334232 resident bytes, 0 undefined, 18655 relocs, 1 ctors, 1 dtors)
ok: nes_band built and verified for 1 target(s)
```

- `readelf -h`：ELF32、little endian、`REL`（ET_REL）、`Machine: ARM`、`Flags: 0x5000200 Version5 EABI, soft-float ABI`，与 target 的 `nuttx-modlib-elf32-rel` / armv8-m.main / soft float 一致。
- 节尺寸：`.text 274524`、`.rodata 59408`、`.data 60`、`.bss 232`、`.init_array 4`、`.fini_array 4`。
- 描述符字符串存在：`nes_band`、`0.1.0`、`nes-band-0.1.0`、`xiaomi-band-10-pro-3.101.043`。

### 3.2 `.canopus` 包（结构与元数据）

`canopus package verify`：`package OK: nes_band v0.1.0 (1 artifact(s))`。

包为 POSIX tar，内容：

```
artifacts/xiaomi-band-10-pro-3.101.043/module.elf   676148 B
manifest.json                                        1361 B
receipts/xiaomi-band-10-pro-3.101.043.cmi              176 B
resources/common/appicon_nes.bin                     9228 B
```

`manifest.json` 关键字段：

- `module_id / package_id = nes_band`，`version = 0.1.0`，`version_code = 1`，`canopus_abi = "2"`；
- `target_id = xiaomi-band-10-pro-3.101.043`，`target_pack_revision = 8`；
- `firmware_sha256 = 519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`（与 target.toml 一致）；
- `lifecycle = resident-after-activation`，`reboot_required = true`；
- `capabilities.required = [identity-guard, native-app.register, launcher.entry, ui.dispatch]`；
- 每个 artifact 记录 ELF `sha256` 与 CMI2 `receipt_sha256`；资源记录 sha256；
- `reproducibility.build_environment = "canopus 0.1.0, sdk 4928f1eb70594263292c32ffc531a7cad3aea555"`。

收据 `receipts/xiaomi-band-10-pro-3.101.043.cmi` 为二进制 CMI2：magic `CMI2`、长度 0xb0、模块名 `nes_band`、target id、固件 sha256 与 ELF sha256。

**签名**：该包格式没有独立签名块/证书，完整性由 manifest 内的逐文件 SHA-256、CMI2 收据和 `canopus package verify` 保证（校验的是哈希与结构，不是密码学签名）。此结论仅描述当前 `canopus 0.1.0` 的包格式，不代表设备安装器会接受。

### 3.3 主机画面（主机验证，非设备验证）

`cargo run --release --manifest-path host/Cargo.toml` 输出：

```
emulator_bytes=76360 rgb565_bytes=122880
panel=336x480 nes=256x240 offset=(40,120)
frame=10 unique_colors=3 fnv=0x2c6c2e45
frame=30 unique_colors=3 fnv=0x53f31e45
frame=60 unique_colors=3 fnv=0xb81577e5
rendered_frames=60 host_us_per_frame=2688
```

逐像素检查（`frame_0010_336x480.ppm`）：

- 屏幕 336×480，四角与边框为纯黑 `(0,0,0)`；
- 画面区域严格为 x∈[40,295]、y∈[120,359]（256×240）：`x=39` / `x=296` / `y=119` / `y=360` 为黑，边界内为绘制色；
- 画面内出现 3 种颜色：`(156,32,33)`、`(74,211,33)`、`(49,48,239)`，与测试 ROM 的 3 色调色板一致；
- frame 10/30/60 的色带位置随 ROM 的逐帧滚动改变（fnv 互不相同），说明帧数据在刷新而不是静态贴图；
- 三种输出（256×240 PPM、336×480 PPM、336×480 PNG）由同一 RGB565 缓冲写出，PNG 为 336×480、8-bit RGB。

结论：**转换路径（调色板 → RGB565 → 居中黑边）在主机侧正确**。设备侧的 LVGL 对象创建、内存图像源与定时刷新尚未验证。

## 4. 证据等级逐项

| 项 | 等级 | 说明 |
| --- | --- | --- |
| `canopus build` / ELF verifier / `package verify` | 编译与静态验证通过 | 本轮实际执行 |
| 模块 ELF 结构、target 绑定、包元数据/哈希 | 编译与静态验证通过 | `readelf`、manifest、CMI2、sha256 |
| 主机 256×240 模拟帧 | 主机验证 | x-nes 实际运行，3 帧快照 |
| 主机 RGB565/居中/黑边转换 | 主机验证 | 与设备共用 `video.rs` |
| `lv_image_create` / `lv_image_set_src`（文件路径源） | DEVICE_PROVEN（沿用 vendor 记录） | LyraPlayer 在 043 上验证；本轮未新增设备证据 |
| `lv_image_set_src` 传内存 `lv_image_dsc_t` | 静态/主机证据 | 标准 LVGL v9 用法，但未在设备探针中验证 |
| `bt_alloc` / `bt_free` 全局分配器 | restricted / approval PENDING（沿用 vendor 记录） | LyraPlayer 同模式；本轮未提升等级 |
| 模块加载、原生应用注册、启动器入口、画面真正上屏 | **设备未验证** | 需用户在小手环上安装后确认 |

## 5. 已知问题与限制

1. **帧率未标定**：定时器 100 ms（约 10 fps），主机 release 2.7 ms/帧；M33 实测缺失，未达到也未声称达到 20–30 fps。若设备上单帧超过 100 ms，UI 线程会被拖慢，需要加跳帧或降低频率。
2. **内存吃紧**：`Emulator 76360 B + NROM 49152 B + RGB565 122880 B ≈ 244 KiB`，远超模块堆 128 KiB，依赖 `bt_alloc`；若设备策略拒绝或池不足，`activate` 会返回 `-2`（NO_MEMORY）。
3. **mapper 覆盖**：只验证 NROM（mapper 0）。其余 mapper 虽能编译，但 ROM 解析与内存占用未验证；大 mapper 可能直接分配失败。
4. **无音频、无按键、无存档**：本轮范围外；`tick()` 仍会推进 APU 状态，只是不输出。
5. **内存图像源未设备验证**：`lv_image_set_src` 传 `lv_image_dsc_t` 是本轮新增用法；若设备 LVGL 版本/缓存策略不兼容，画面可能不刷新。回退方案是写 `/data` 下的 LVGL BIN 文件并按路径设置源（DEVICE_PROVEN 用法），但会引入文件 I/O 与写放大。
6. **`.rodata` 偏大**：59 KiB，主要是 core/panic 格式化字符串与路径；可后续用 `panic=immediate-abort`、去除格式化路径等收窄。
7. **`lv_display_invalidate_area` 不可用**：private 层未导出 wrapper，只能靠双描述符交替触发失效；若 LVGL 仍判定同图不失效，需要改为改尺寸/重设对象等替代手段。
8. **安装未执行**：本轮没有也不能执行设备安装；“编译通过 / 主机验证”不等于“设备可运行”。

## 6. 设备侧待验证项与安装方式

待验证：

1. 模块能被 Supervisor 加载（`canopus_mod_prepare/activate` 返回 0，无 `-2` 内存错误）。
2. 两阶段原生应用注册成功（stage 1 `app_install` + `app_lookup`，stage 2 `launcher_add`），启动器出现 “NES” 入口。
3. 打开页面后 336×480 面板中央出现 256×240 画面、四周黑边，且画面随帧变化（测试 ROM 色带滚动）。
4. 单帧耗时与 UI 响应；必要时调整 `FRAME_PERIOD_MS`。
5. `bt_alloc` 在设备上是否被批准、244 KiB 分配是否成功。
6. 返回/离开页面时定时器与对象销毁是否干净（无泄漏、无崩溃）。

安装方式（由用户执行，本轮未执行）：将 `dist/nes_band-0.1.0.canopus` 通过 Canopus 的安装渠道导入设备（Canopus Manager/安装器），重启或按 Supervisor 提示加载；可用 `canopus_mod_query` 的 status 值（MAGIC `NES1`、帧数、指令数）确认运行状态。

## 7. 依据版本

- 基址仓库 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`
- Canopus：`4928f1eb70594263292c32ffc531a7cad3aea555`
- Canopus-App-LyraPlayer：`8b228e37157766e0ea7e3d31a9d0ca2aaf053d7f`
- x-nes：`ca4c3955b8a442fe25ee62abd57fca6849d1e70b`
- Mi8WfBinTool：`5e4195ce91b9d8810a7a2233c96b4cba44184adc`
- 工具链：`rustc 1.97.0 (2d8144b78 2026-07-07)`，`canopus 0.1.0`
