# REPORT — 小米手环 10 Pro（043）NES 模拟器画面输出

- 执行日期：2026-11-01
- 仓库/分支：`https://github.com/oraserz/workspace.git`，`trae/agent-GbKpGn`
- 验证环境：本仓库的 QEMU `bes1503` 模型 + 真实固件 043（`models/band10pro`），VNC 截图见 `/workspace/verify/nespro/`
- 约束：未修改 Canopus 模块管理器源码（`vendor/*` 仅为指向 `/workspace/.projects/*` 的符号链接，未提交改动）；未向 QEMU 仓库提交任何内容

## 1. 结论

| 验收项 | 状态 | 证据等级 |
| --- | --- | --- |
| 启用模块后不再卡死 | 达成 | **模拟器实机验证**（真实固件 043） |
| 应用入口出现在 launcher，可打开 | 达成 | **模拟器实机验证**（应用列表最底部，罗盘图标） |
| 页面创建并显示 NES 画面 | 达成 | **模拟器实机验证**（多色棋盘帧，持续刷新） |
| 页面可退出（右滑） | 达成 | **模拟器实机验证** |
| 构建可复现、包结构可校验 | 达成 | 编译/包校验 |

说明：模拟器验证属于本项目内部的 host/emulator 级证据；物理真机尚未复测。

## 2. 交付物

| 交付物 | 路径 | 校验 |
| --- | --- | --- |
| 可安装模块包 | `deliverables/nespro-0.1.0.canopus` | 487,424 B；sha256 `2b129e1dca3bbb57bb794f1bb7bf9c5bc6814e4a5110eec365a41897c9ae1510`；`canopus package verify` → `package OK` |
| 模块 ELF | `nespro/build/xiaomi-band-10-pro-3.101.043/nespro.elf` | sha256 `2f1c201e41d153d601eda90848fefc26a8fb591e53b65a5b3141ab7940b4a76a`；`PASS (15 sections, 451241 resident bytes, 0 undefined, 15270 relocs, 1 ctors, 1 dtors)` |
| Launcher 安装器 | `deliverables/nespro-installer-xiaomi-band-10-pro/`、`.zip` | 内含新 ELF/CMI 与 `canopus package --installer` 生成的 `main.lua` |
| 自产测试 ROM | `roms/nespro-test.nes` | NROM-128 / mapper 0 / CHR RAM；4 色板 × 棋盘/纯色 tile |
| 验证截图 | `/workspace/verify/nespro/*.png` | `final_nes_page.png`（NES 画面）、`final_exit.png`（右滑退出后回到表盘） |

`analysis/mi8/out/` 是资源解包产物，仅供本地分析，不属于交付物。

## 3. 复现构建

```bash
export CANOPUS_ROOT=/workspace/nes-band10pro/vendor/canopus
python3 tools/make_test_rom.py roms/nespro-test.nes
cd nespro
canopus build                 # → build/xiaomi-band-10-pro-3.101.043/nespro.elf
canopus package --build       # → dist/nespro-0.1.0.canopus
canopus package verify dist/nespro-0.1.0.canopus
```

## 4. 卡死根因与修复

`page_on_create` 运行在固件 UI 任务上，栈只有几 KiB。原实现：

- `Emulator::new(...)` 按值返回 76,288 B；
- `Mapper::from_ines` 内联构建 48 KiB 结构；
- `Nrom::new` 另需 40 KiB。

三者叠加导致 UI 任务栈溢出 → 启用模块即卡死/页面无法打开。

修复（`nespro/src/lib.rs`）：

1. `emulator_init_in_place`：对 `MaybeUninit<Emulator>` 先 `write_bytes(0)`，再用裸指针逐字段写入（`mapper`、`ppu.odd_frame`、`apu` 采样率、`CpuRp2a03::new(0)`），最后 `reset()`；不再按值构造。
2. `nespro/src/cart.rs`：自建 `Cart`，用 `Vec<u8>` 持有 PRG/CHR，只把两份指针移入 `Mapper`，规避 40/48 KiB 栈帧。
3. 页面仍保留：内容容器、`lv_image`、`lv_image_dsc_t`、33 ms LVGL 定时器；首帧在 `page_on_create` 内立即绘制，避免空白页。

## 5. 模拟器实机验证（QEMU + 固件 043）

验证表盘（lua，放在 `/data`，不属于模块交付物）经 `/dev/canopus` 依次执行：`insmod` → `INSTALL` → `ENABLE` → `ACTIVATE` → `NATIVE_STAGE 1/2`。UART 原样输出：

```
[nespro-verify] verification watchface started
[nespro-verify] removed registry.bin ok=true
[nespro-verify] supervisor loaded
[nespro-verify] step 1 ok: 加载 Canopus 运行环境
[nespro-verify] step 2 ok: 安装 nespro 模块
[nespro-verify] step 3 ok: 启用模块（写入启动意图）
[nespro-verify] after activate: id=nespro state=9 version=1 flags=0 activation=0 modules=1 safe=0 error=0
[nespro-verify] step 4 ok: 激活模块并加载镜像
[nespro-verify] step 5 ok: 注册 NES 应用入口
[nespro-verify] step 6 ok: 打开 NES 应用页面
```

`state=9` 即 `CANOPUS_STATE_ACTIVE`：模块已加载并激活，未出现卡死或重启。

模块侧探针（验证用临时构建，最终包已移除，避免每 2 s 写 flash 的磨损）：

- `trace.stage1.id = 204`：`launcher.register_app` 返回的 app id；
- `trace.create` / `trace.resume` / `trace.frame`：页面创建、恢复、首帧绘制均执行；
- `trace.frames`：帧计数在该页面打开期间由 60 递增到 120，说明 33 ms 定时器持续运行；
- `trace.goto.ui rc = 0`：stage-2 通过 `ctx->ui->post` 在 UI 任务上调用 `page_goto` 成功。

画面：`/workspace/verify/nespro/final_nes_page.png` 为内嵌测试 ROM 的 4 组调色板棋盘（256×240 → RGB565 → 336 宽缩放，下方黑边为页面剩余区域）；`final_exit.png` 为右滑退出后回到表盘。

## 6. 已知问题与限制

1. **模拟器 registry 限制**：`sup_registry_persist` 使用 tmp+rename，模拟器 flash journal 不保证 rename，`registry.bin` 常为 0 字节。验证表盘在启动前 `os.remove("/data/canopus/registry.bin")`；**仅模拟器需要**，真机 rename 原子。
2. **stage-2 自动导航**：调用成功且页面被创建，但 lua 验证表盘在随后仍操作 UI，页面会被系统回收；可靠入口是 launcher 图标（应用列表最底部）。真实表盘环境下可能表现不同，需真机复核。
3. **ROM 体积**：保留 NROM/UxROM/CNROM；受 128 KiB 模块堆限制，大 ROM 不支持。
4. **音频/按键/存档**：未实现。
5. **包签名**：receipt 为未签名 CMI2。

## 7. 下一步

1. 物理真机安装 `deliverables/nespro-0.1.0.canopus` 复核（真机 rename/调度与模拟器略有差异）。
2. 放入 `/data/canopus/nespro.nes` 验证 ROM 文件加载路径。
3. 如需“启用即自动进入页面”，在真实表盘环境复测 stage-2；或改由模块自有 LVGL 定时器延后调用。
