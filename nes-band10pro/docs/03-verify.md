# 阶段 3 验证报告：NES 帧输出路径与交付物

- 执行日期：2026-10-27（Asia/Shanghai）
- 基址仓库：`https://github.com/oraserz/workspace.git`，commit `5df98ba6d1a8c5351b289e878e29c1c30f21d524`
- 目标：`xiaomi-band-10-pro-3.101.043`（armv8-m.main / Cortex-M33 / Thumb2 / soft float）
- 本轮定位：阶段 2 已在此前会话完成；本次重新拉取基址、重建 vendor、复跑构建/打包，并按 `prompts/03-verify.md` 做主机侧与包结构验证。

> 证据等级：`BUILD_ONLY` / `HOST_TESTED` / **设备未验证**。本文件不声称设备已能运行。

---

## 1. 环境复核

| 工具 | 版本 | 说明 |
|---|---|---|
| rustc / cargo | 1.97.0 (2d8144b78 2026-07-07) | `+1.97.0`，已装 `thumbv8m.main-none-eabi` |
| canopus CLI | 0.1.0（源码构建） | `vendor/Canopus/target/release/canopus` |
| clang / LLD | 17.0.0 | Cortex-M33 / soft float |
| Python | 3.14.8 | 测试 ROM 生成、PNG 转换、缩放合成 |
| 主机 | Linux x86_64 沙箱 | 无 TTY，非交互 |

`canopus doctor`：SDK / targets / C compiler / linker / archiver / curl / python3 全部 ok；Rust 一项仍是默认工具链 1.92.0 的提示，本工程显式用 1.97.0，不影响构建。

vendor commit：

| 仓库 | commit |
|---|---|
| Canopus | `4928f1eb70594263292c32ffc531a7cad3aea555` |
| x-nes | `ca4c3955b8a442fe25ee62abd57fca6849d1e70b` |
| Canopus-App-LyraPlayer | `8b228e37157766e0ea7e3d31a9d0ca2aaf053d7f` |
| Mi8WfBinTool | `5e4195ce91b9d8810a7a2233c96b4cba44184adc` |

x-nes 的本地补丁已导出为 `patches/x-nes-043.patch`（12 文件、+63/−2，sha256 `a4b3e3536297bd4c297190926bfaad047d62d073fd987d2974d029722cf33900`）。

---

## 2. 构建与打包（BUILD_ONLY）

```bash
cd /workspace/nes-band10pro/vendor/Canopus && cargo build --release -p canopus-cli
cd /workspace/nes-band10pro/nesband10pro
RUSTUP_TOOLCHAIN=1.97.0 CARGO=/root/.cargo/bin/cargo \
  /workspace/nes-band10pro/tools/bin/canopus build
RUSTUP_TOOLCHAIN=1.97.0 CARGO=/root/.cargo/bin/cargo \
  /workspace/nes-band10pro/tools/bin/canopus package
```

实测：

```
built ./build/xiaomi-band-10-pro-3.101.043/nesband10pro.elf
sha256: 39ce46610c5db68061d8a0f4498a43f62c9decd1890b06db0503aaafc47cf97c
summary: PASS (14 sections, 311256 resident bytes, 0 undefined, 18700 relocs, 1 ctors, 1 dtors)

wrote ./dist/nesband10pro-0.1.0.canopus (661504 bytes, 1 target(s), versionCode 1)
```

- 包 SHA-256：`9c42201373d3c234cc566ace79508b9326617d2005889a0680c6abfe730ea61f`
- `.text` 275,476 / `.rodata` 35,576 / `.bss` 196 / `.init_array` 4 / `.fini_array` 4
- `0 undefined`：没有外部 libc 导入；帧缓冲不在静态 BSS。

---

## 3. `.canopus` 包结构（校验通过）

`dist/nesband10pro-0.1.0.canopus` 是未压缩 POSIX tar，条目：

```
artifacts/xiaomi-band-10-pro-3.101.043/module.elf   656,992 B
manifest.json                                          992 B
receipts/xiaomi-band-10-pro-3.101.043.cmi             176 B
```

`manifest.json` 关键字段：

- `module_id` / `package_id` = `nesband10pro`
- `version` = `0.1.0`，`version_code` = 1，`canopus_abi` = `"2"`，`lifecycle` = `"removable"`
- `target_id` = `xiaomi-band-10-pro-3.101.043`，`target_pack_revision` = 8（与 `targets/.../target.toml` 的 `revision = 8` 一致）
- `sha256` = `39ce46610c5db68061d8a0f4498a43f62c9decd1890b06db0503aaafc47cf97c`（与 ELF 实测一致）
- `receipt_sha256` = `dd247bbcb50db8809a25b40bd266af55f53b98d5f039f7f4dc8c34177e9cdaac`
- `firmware_sha256` = `519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`（target pack 绑定值）
- `reproducibility.build_environment` = `canopus 0.1.0, sdk 4928f1eb...`

`canopus package verify` 输出 `package OK: nesband10pro v0.1.0 (1 artifact(s))`。

**签名说明**：该包没有密码学签名条目；完整性由 manifest 的 SHA-256 + CMI receipt 校验，`package verify` 按这些哈希复核。未伪造或声称存在签名。

---

## 4. 主机侧画面验证（HOST_TESTED）

### 4.1 同一转换路径

`probe/host-render` 通过 `#[path = "../../../nesband10pro/src/frame.rs"]` 编译设备端同一份 `convert_raw`，把 x-nes 的 256×240 调色板索引帧转为 128×96 RGB565。

```bash
cd /workspace/nes-band10pro
./probe/host-render/target/release/host-render assets/pattern.nes build/host-render
python3 scripts/ppm_to_png.py build/host-render/frame8.ppm build/host-render/frame8.png
```

实测：

```
frame 1: 29780 cycles, unique colours 1 -> 1
frame 2: 29783 cycles, unique colours 2 -> 2
frame 8: 29781 cycles, unique colours 3 -> 3
ppu: mask=0x0a ctrl=0x08 v=0x1001 t=0x0000 render_v=0x0000 fine_x=0
in-place conversion matches separate buffer: true
frames=8 snapshots=3 cycles/frame=29780 check=true
```

- `check=true`：32×32 色块循环等于 `[0x16, 0x2A, 0x12][(bx+by)%3]`，第 0 行就是棋盘格。
- `in-place ... true`：源地址故意为奇数（`+57`），就地转换与独立缓冲逐字节一致。
- 目视：`build/host-render/frame8.png` 为红/黄/蓝三色棋盘格，尺寸 128×96，无裁切、无错行。

### 4.2 整数倍缩放与黑边

按设备端 `layout()` 同一参数（336×480、`CONTENT_TOP=56`、2×）合成 336×480 预览：

```
128×96 --2×--> 256×192
origin (40, 172)；非黑边界 (40,172)-(295,363)；左右黑边 40 px，上 172 px，下 116 px
```

产物：`build/host-render/frame8-screen.ppm` / `frame8-screen.png`（336×480）。目视确认棋盘格完整、居中、无裁切。

> 该合成使用与设备相同的整数缩放参数，但 LVGL 实际绘制仍是设备待验证项。

---

## 5. 证据等级

| 项目 | 等级 | 依据 |
|---|---|---|
| `canopus build` + ELF verifier PASS | `BUILD_ONLY` | 本次实测 |
| `canopus package` + `package verify` | `BUILD_ONLY` | 本次实测 |
| 测试 ROM 生成可复现 | `HOST_TESTED` | 脚本两次输出同一 SHA-256 `aa2cf2...` |
| x-nes 跑帧 + 同一 `frame.rs` 转换 | `HOST_TESTED` | `check=true`、PNG 目视 |
| 2× 整数缩放 / 黑边布局 | `HOST_TESTED` | 336×480 合成预览 |
| RGB565 descriptor 在真机被接受 | **未验证** | `BUILD_ONLY`，需设备探针 |
| 启动器页面能打开、A/B 重绘、真实帧率/堆余量 | **未验证** | 需设备探针 |

---

## 6. 已知问题与设备待验证项

1. `LV_COLOR_FORMAT_RGB565 = 0x12` 仍是 LVGL v9 上游推断，未在 043 固件验证；花屏/全黑时优先怀疑此项。
2. RGB565 字节序未验证（若红蓝互换，需交换字节或改 BGR565）。
3. 模块堆内变量图 descriptor 是否被固件接受、A/B descriptor 交替是否触发重绘，均未验证。
4. `lvx_image_set_scale` 的 Q8 语义来自绑定注释/LyraPlayer，2× 实际绘制未验证。
5. `ctx->display->get_resolution` 已接入，但 `.043` 返回值未在设备读取；回退为 336×480。
6. 帧率按 50 ms 定时器设计（显示 20fps），真实耗时与 `query[4]` 未在设备读取。
7. 仅支持 mapper 0（NROM）；音频、按键、存档未实现。

设备侧逐项检查清单见 `docs/02-implementation.md` 第 10 节与 `REPORT.md` 第 5 节。
