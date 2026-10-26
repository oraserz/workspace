# REPORT：小米手环 10 Pro（043）NES 模拟器（画面输出）

- 日期：2026-10-27
- 基址仓库：`https://github.com/oraserz/workspace.git`
- 基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`
- 目标：`xiaomi-band-10-pro-3.101.043`（armv8-m.main / Cortex-M33 / Thumb2 / soft float）
- 本轮状态：阶段 1、2 已完成；阶段 3 主机侧验证与交付收尾完成；**设备未验证**

---

## 1. 结论摘要

1. 正式模块 `nesband10pro` 可构建、可通过 Canopus ELF verifier、可打包为 `.canopus`。
2. 上屏路径明确：x-nes 逐帧模拟 → 256×240 调色板索引帧 → 就地转 128×96 RGB565 → LVGL v9 内存 image descriptor → `lvx_image_set_src` 刷新原生页面 image widget。
3. 测试 ROM 由脚本生成（无版权内容），主机侧跑帧与同一 `frame.rs` 转换通过，PNG 目视正确。
4. 屏幕尺寸运行时取 `ctx->display->get_resolution`；默认 336×480 下用 2× 整数缩放（256×192）并居中留黑边。
5. **没有在真机验证**：页面能否打开、RGB565 descriptor 是否被接受、字节序、A/B 重绘、真实帧率与堆余量都待用户设备探针。

---

## 2. 一键复现构建

```bash
/workspace/nes-band10pro/scripts/build.sh
```

脚本会：

1. 按固定 commit 拉取 vendor（Canopus / x-nes / LyraPlayer / Mi8WfBinTool，已存在则 checkout）；
2. 应用 `patches/x-nes-043.patch`（幂等）；
3. 构建 Canopus CLI，生成 `tools/bin/canopus` 符号链接；
4. 运行 `scripts/gen_test_rom.py` 生成 `assets/pattern.nes`；
5. `canopus build` + `canopus package` + `canopus package verify`；
6. 构建 `probe/host-render`，跑 8 帧、导出 PPM/PNG，并打印 SHA-256。

前置条件：Rust 1.97.0 + `thumbv8m.main-none-eabi`（脚本用 `RUSTUP_TOOLCHAIN`/`CARGO` 指定）、clang/LLD 17、Python 3。

手动等价命令见 `docs/02-implementation.md` 第 9 节与 `docs/03-verify.md` 第 2 节。

---

## 3. 产物绝对路径

| 产物 | 绝对路径 | SHA-256 |
|---|---|---|
| 可安装包 | `/workspace/nes-band10pro/nesband10pro/dist/nesband10pro-0.1.0.canopus` | `9c42201373d3c234cc566ace79508b9326617d2005889a0680c6abfe730ea61f` |
| 模块 ELF | `/workspace/nes-band10pro/nesband10pro/build/xiaomi-band-10-pro-3.101.043/nesband10pro.elf` | `39ce46610c5db68061d8a0f4498a43f62c9decd1890b06db0503aaafc47cf97c` |
| 测试 ROM | `/workspace/nes-band10pro/assets/pattern.nes` | `aa2cf266a2953dbee9e3729f5edbc694c69d2148eed1a3e2d4c1d09e0f96b204` |
| x-nes 补丁 | `/workspace/nes-band10pro/patches/x-nes-043.patch` | `a4b3e3536297bd4c297190926bfaad047d62d073fd987d2974d029722cf33900` |
| 主机帧（128×96） | `/workspace/nes-band10pro/build/host-render/frame8.png` | `67b69ccc654b2864e084f28086a176fd5d6dbce391be7fbe548c7ae99c82e838` |
| 屏幕合成（336×480） | `/workspace/nes-band10pro/build/host-render/frame8-screen.png` | `64fcdcec4d225c0ea8fef6216ea4d9f373ce6be627b67b9636f5012343cfc79e` |
| 实现文档 | `/workspace/nes-band10pro/docs/02-implementation.md` | — |
| 验证文档 | `/workspace/nes-band10pro/docs/03-verify.md` | — |
| 决策文档 | `/workspace/nes-band10pro/docs/DECISION.md` | — |
| 状态文件 | `/workspace/nes-band10pro/STATE.md` | — |

包内容：`artifacts/xiaomi-band-10-pro-3.101.043/module.elf`、`manifest.json`、`receipts/xiaomi-band-10-pro-3.101.043.cmi`。
manifest 的 `target_id` 为 `xiaomi-band-10-pro-3.101.043`、`target_pack_revision` 为 8，与 target pack 一致；`package verify` 通过。包内没有密码学签名，完整性与绑定由 SHA-256 + CMI receipt 校验。

---

## 4. 证据等级

| 项目 | 等级 | 说明 |
|---|---|---|
| `canopus build` + ELF verifier | `BUILD_ONLY` | 14 sections / 311,256 resident / 0 undefined |
| `canopus package` + `package verify` | `BUILD_ONLY` | 661,504 B，1 target |
| 测试 ROM 生成 | `HOST_TESTED` | 两次生成 SHA-256 相同 |
| x-nes 跑帧 + 设备同一 `frame.rs` 转换 | `HOST_TESTED` | `check=true`、`cycles/frame=29780`、PNG 目视 |
| 2× 整数缩放/黑边 | `HOST_TESTED` | 336×480 合成，256×192 居中 |
| 真机显示、页面注册、RGB565 接受度、帧率、堆余量 | **设备未验证** | 需用户执行 |

“编译通过 / 主机测试 / 设备验证”严格分开；本报告不把前两者写成设备验证。

---

## 5. 设备侧待验证（用户执行）

1. `insmod` 后启动器是否出现 “NES” 条目与 compass 图标（`app_id=0x4E45`）。
2. 打开 `nes_player` 页，image widget 是否出现并显示棋盘格。
3. `LV_COLOR_FORMAT_RGB565 = 0x12` 是否被固件接受；若花屏/全黑，优先怀疑此项（回退见 `DECISION.md` D6）。
4. RGB565 字节序：若红蓝互换，需交换字节或改 BGR565。
5. `lvx_image_set_scale(512)` 是否按 2× 放大；A/B descriptor 交替是否触发重绘。
6. `ctx->display->get_resolution` 返回值是否与 336×480 一致。
7. 读 `query`：`[4]` last_frame_ms、`[7]/[8]` heap_used/available，核对帧率与 125,544 B 堆峰值核算。
8. ROM 放置：`pattern.nes` 推到 `/data/canopus/m.nesband10pro/pattern.nes`（或模块 `get_data_path` 目录），重启模块后打开页面。

---

## 6. 已知问题与限制

- 只支持 mapper 0（NROM）；其他 mapper 返回 `-1000 - id`，不静默错绘。
- 音频、按键、存档未实现。
- 显示固定 20 fps 定时器（50 ms），不做追帧；真实性能未测。
- 128 KiB 模块堆预算下，`Emulator` 76,360 B + `Nrom` 49,168 B + 块头 16 B ≈ 125,544 B，余量约 5.5 KiB，偏紧。
- 变量图 descriptor、RGB565、`lvx_image_set_scale` 的实际固件行为均未设备验证。
- `get_resolution` 服务不可用或返回 0 时回退 336×480。

---

## 7. 依据与仓库

| 仓库 | commit |
|---|---|
| 基址 `workspace` | `5df98ba6d1a8c5351b289e878e29c1c30f21d524`（未推送） |
| Canopus | `4928f1eb70594263292c32ffc531a7cad3aea555` |
| x-nes | `ca4c3955b8a442fe25ee62abd57fca6849d1e70b` + 本地补丁 |
| Canopus-App-LyraPlayer | `8b228e37157766e0ea7e3d31a9d0ca2aaf053d7f` |
| Mi8WfBinTool | `5e4195ce91b9d8810a7a2233c96b4cba44184adc` |

---

## 8. 边界声明

- 未向基址仓库推送任何内容；基址仅只读使用。
- 未提交任何版权 ROM 或固件 `bin`；`assets/pattern.nes` 由 `scripts/gen_test_rom.py` 自生成。
- 未跨 target 推断固件地址；未提升任何 symbols 证据等级。
- 设备侧行为未验证，安装与真机测试由用户完成。
