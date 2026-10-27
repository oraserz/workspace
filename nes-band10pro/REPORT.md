# REPORT — 小米手环 10 Pro（043）NES 模拟器画面输出

- 执行日期：2026-10-27（日期守卫通过；`/workspace/nes-band10pro/.run.lock` 不存在）
- 依据基址 commit：`5df98ba6d1a8c5351b289e878e29c1c30f21d524`
  （`https://github.com/oraserz/workspace.git`，`pull --ff-only`：Already up to date）
- vendor：Canopus `4928f1eb70594263292c32ffc531a7cad3aea555`、x-nes `ca4c3955b8a442fe25ee62abd57fca6849d1e70b`
- 构建环境：rustc/cargo 1.92.0、clang 17.0.0、LLD 17.0.0、canopus CLI 0.1.0、Python 3.14.8

## 1. 结论（证据等级）

| 验收项 | 状态 | 证据等级 |
| --- | --- | --- |
| 工程可一键复现构建 | 达成 | **编译通过** |
| `.canopus` 包存在并过 verifier / package verify | 达成 | **编译通过**（结构+哈希，不是设备签名） |
| 主机侧看到测试 ROM 的正确 NES 画面 | 达成 | **主机验证** |
| 文档区分证据等级、不声称设备可用 | 达成 | 本文件 + `docs/` |
| 真机安装与上屏 | 未做 | **设备未验证** |

**没有任何设备验证结论。** 所有固件私有入口（页面 descriptor、LVGL `lv_image_*`、`lvx_*`）仍是 STATIC_RECOVERED，未提升证据等级。

## 2. 交付物绝对路径

| 交付物 | 路径 | 校验 |
| --- | --- | --- |
| 可安装模块包 | `/workspace/nes-band10pro/nespro/dist/nespro-0.1.0.canopus` | 685,056 B；sha256 `192991909062054b10457e460dd121d57ee1278d221243affd4e2e5186515b4f`；`canopus package verify` → `package OK: nespro v0.1.0 (1 artifact(s))` |
| 模块 ELF | `/workspace/nes-band10pro/nespro/build/xiaomi-band-10-pro-3.101.043/nespro.elf` | sha256 `289cbec389c1412d7e918bd8f35ca943ba2ea5594f3e3a07546c8d5d0cf8bb57`；verifier `PASS (15 sections, 515869 resident bytes, 0 undefined, 19919 relocs, 1 ctors, 1 dtors)` |
| 自产测试 ROM | `/workspace/nes-band10pro/roms/nespro-test.nes` | 16,400 B；sha256 `e744659ea187154933d273f08c5f31cb719c1dad92834ec5cdac1606acffe101`；NROM-128 / mapper 0 / CHR RAM |
| 主机验证帧 | `/workspace/nes-band10pro/hostverify/out/frame-04.ppm`、`frame-04.png` | 256×240，13 色 |
| 设备布局预览 | `/workspace/nes-band10pro/hostverify/out/content-424.ppm`、`content-424.png` | 336×424，画面贴顶 336×315 + 下方黑边 |
| 调研文档 | `/workspace/nes-band10pro/docs/01-research.md` | — |
| 决策文档 | `/workspace/nes-band10pro/docs/DECISION.md` | — |
| 实现文档 | `/workspace/nes-band10pro/docs/02-implementation.md` | — |
| 状态文件 | `/workspace/nes-band10pro/STATE.md` | — |

`analysis/mi8/out/` 是 `resource.bin` 的解包分析产物（含表盘美术资源），**仅供本地分析，不属于交付物，不得发布或复用**。

## 3. 复现构建

```bash
export CANOPUS_ROOT=/workspace/nes-band10pro/vendor/canopus

# 测试 ROM（已生成，可重复）
python3 /workspace/nes-band10pro/tools/make_test_rom.py \
  /workspace/nes-band10pro/roms/nespro-test.nes

cd /workspace/nes-band10pro/nespro
canopus build                 # → build/xiaomi-band-10-pro-3.101.043/nespro.elf + verifier
canopus package --build       # → dist/nespro-0.1.0.canopus
canopus package verify dist/nespro-0.1.0.canopus

# 主机画面（可选）
cd /workspace/nes-band10pro/hostverify
cargo run --release -- ../roms/nespro-test.nes out
```

## 4. 验证证据摘要

### 4.1 编译与链接

- `canopus doctor`：SDK / targets（8 active，含 3.101.043）/ clang / LLD / Rust（thumbv8m.main-none-eabi）/ llvm-ar / python3 全 ok。
- `canopus build`：cargo staticlib → C ctor shim → `ld.lld -r` → ELF verifier PASS。
- 可复现性：`canopus clean`（移除 build/dist）后重新 `canopus build`，ELF sha256 仍为 `289cbec3...`；重新 `canopus package` 后包 sha256 仍为 `1929919...`，两次完全相同。
- x-nes 确实链接：设备 ELF 内 `llvm-nm | grep -c '_ZN3nes'` 计得 402 个符号，verifier 报 0 undefined。
- 内存静态核算（主机实测）：`size_of::<nes::Emulator>() = 76,360`、`size_of::<Bus>() = 76,352`；16 KiB ROM 的 mapper 自持 PRG 16,384 + CHR 8,192；稳态约 100,936 B / 131,072 B 模块堆。

### 4.2 包结构

`.canopus` 为 tar：

```
artifacts/xiaomi-band-10-pro-3.101.043/module.elf   680,616 B  sha256 = 289cbec3...
manifest.json                                      module_id nespro / version 0.1.0 / abi 2 / target_pack_revision 8
receipts/xiaomi-band-10-pro-3.101.043.cmi          CMI2 摘要 176 B（未签名）
```

`manifest.json` 的 `firmware_sha256 = 519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec` 与 target.toml 一致；payload ELF 的 sha256 与构建产物一致。

### 4.3 主机画面

- `hostverify` 用与设备**完全相同的** `nespro/src/nes_frame.rs` 做调色板→RGB565 转换。
- 测试 ROM 跑 4 帧、37,632 条指令；`frame-04.ppm` 含 **13 种颜色**（黑/红/橙/蓝/绿/白等），呈现 4 组调色板 × 棋盘/纯色 tile 的循环图案；目视确认无全黑/单色异常。
- `content-424.ppm` 为 336×424（画面 336×315 贴顶 + 下方 50,672 个黑边像素）的设备布局预览；缩放为最近邻近似，**不代表设备 LVGL transform 的实际结果**。

## 5. 本次修补（相对上次中断点）

1. 修复 x-nes 作为设备 rlib 依赖的编译问题（vendor 本地补丁，见 `docs/01-research.md` §6）：
   - `crate-type` 收窄为 `["lib"]`；
   - 5 个 konami mapper 文件补 `use alloc::vec::Vec;`、`common.rs` 补 `use alloc::vec;`；
   - `rust-version` 1.97.0 → 1.85。
2. 修复 `nespro` 编译：crate 名 `nes`（非 `x_nes`）、`status_put_u32`/`status_writer_publish` 改从 `canopus_runtime` 引入、`checked_add` 的 `Option` 解构、`init_dsc` 数组遍历。
3. 接入 `ctx->display->get_resolution()` 运行时校验屏幕尺寸（不再只依赖常量）。
4. 内存峰值优化：`create_mapper()` 后 `drop(rom)` 再构造 76 KiB `Emulator`。
5. 测试 ROM 从 32 KiB 降为 16 KiB PRG（NROM-128 镜像），为 128 KiB 模块堆留出约 30 KiB 余量；画面与指令数不变（4 帧 / 37,632 条）。
6. 首次产出 `docs/01-research.md`、`docs/DECISION.md`、`docs/02-implementation.md`、`STATE.md`、本报告。

## 6. 已知问题与风险

1. **设备未验证**：`register_app`、`firmware_page_descriptor` 布局、`lvx_content_create`、`lv_image_create/set_src/set_scale`、`lvx_timer_create` 全部依赖 STATIC_RECOVERED 固件入口，真机行为未知。
2. **栈风险**：`Emulator::new` 按值构造 76,360 B 结构，模块线程栈大小未知，存在栈溢出风险。
3. **ROM 体积上限**：约 24 KiB 的 PRG/CHR 总量；更大 ROM 会在 `GlobalAlloc` 返回 null 后失败（当前 `start_emulator` 返回 false，页面报错退出，无降级）。
4. **刷新机制**：单帧缓冲 + descriptor 交替 `set_src` 触发重绘；若 LVGL 因指针/内容未变而跳过重绘，可能出现静止画面。回退方案见 `DECISION.md` D2。
5. **缩放/裁剪**：`lv_image_set_scale(336,336)` 为 1.3125×，主机侧仅最近邻预览。
6. **包签名**：receipt 为未签名 CMI2；设备侧安装/授权流程未验证。
7. `cargo` 对 6 处 `static mut` 引用给出 Rust 2024 迁移 warning，不影响构建。

## 7. 设备侧待验证项（用户执行）

1. 将 `nespro-0.1.0.canopus` 按设备安装流程装入，`insmod` 后确认模块加载成功、`query` 返回 `NESP`。
2. 确认 launcher 出现 “NES” 入口，进入后页面创建成功（日志应打印 `display 336x480`）。
3. 确认内嵌测试 ROM 的棋盘/多色画面出现在 336×315 区域，且 33 ms 定时器持续刷新（观察是否卡死、花屏、错位、栈溢出/重启）。
4. 放入 `/data/canopus/nespro.nes`（小体积 NROM）验证文件加载路径。

## 8. 未完成项与下一步

| 项 | 状态 | 下一步 |
| --- | --- | --- |
| 真机安装/上屏 | 未完成（本轮范围外，需用户设备） | 按 §7 验证；若栈溢出，改为静态原地初始化 `Bus`；若 OOM，缩小 ROM 或改为按 bank 读取 |
| 音频 / 按键 / 存档 | 未开始（明确超出本轮） | 后续阶段 |
| 大 ROM 支持 | 不支持 | 按 bank 流式读取或裁剪 x-nes mapper 集合 |
| 真双缓冲 / 局部刷新 | 未做 | 双 `FB` + 仅在帧完成时切换；或寻找可导出的区域失效接口（当前 `lv_display_invalidate_area` 未导出） |
| 帧率实测 | 未测 | 设备侧计时 120k 指令预算，按需降到 40k–60k |
