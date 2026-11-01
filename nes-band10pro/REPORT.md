# REPORT — 小米手环 10 Pro（043）NES 模拟器画面输出

- 执行日期：2026-11-01
- 仓库/分支：`https://github.com/oraserz/workspace.git`，`trae/agent-GbKpGn`
- 约束：未修改 Canopus 模块管理器源码；未向 QEMU 仓库提交任何内容

## 1. 结论

| 验收项 | 状态 | 证据等级 |
| --- | --- | --- |
| 启用模块后不再卡死 | 达成 | **模拟器实机验证**（真实固件 043） |
| launcher 入口可打开、可右滑退出 | 达成 | **模拟器实机验证** |
| 页面显示 NES 画面 | 达成 | **模拟器实机验证**（内嵌 NROM 测试 ROM） |
| UI 线程不再承担模拟/转换 | 已改为定时器任务 + `ui.post` | 编译 + 静态审查，**真机待测** |
| 真机卡顿（响应慢） | 已针对 UI 线程负载做结构性修改 | **真机待测**（用户自测） |

## 2. 交付物

| 交付物 | 路径 | 校验 |
| --- | --- | --- |
| 可安装模块包 | `deliverables/nespro-0.1.2.canopus` | 497,664 B；sha256 `2fa42ee0187f7b73b1b9ae61fff91740637af4161fbddd34101f3eb7d542672b`；`package verify` → `package OK` |
| 模块 ELF | `nespro/build/xiaomi-band-10-pro-3.101.043/nespro.elf` | sha256 `b26fa5bb743d710c808519238214f6067fd7e98825872abfaf6d817b9a20e387`；`PASS (15 sections, 455416 resident bytes, 0 undefined, 15385 relocs, 1 ctors, 1 dtors)` |
| Launcher 安装器 | `deliverables/nespro-installer-xiaomi-band-10-pro/`、`.zip` | 含新 ELF/CMI |
| 自产测试 ROM | `roms/nespro-test.nes` | NROM-128 / mapper 0 / CHR RAM |
| 期望画面脚本 | `/workspace/verify/nespro/make_expected.py` | 输出 256×240 / 336×315 / 整页 PNG，13 色 |

**版本号提升到 0.1.2**：同版本重复安装会被 Supervisor 判 `-105 STAGE_DUPLICATE`，不会替换已安装的旧二进制。

## 3. 卡死根因（已修复）

`page_on_create` 在固件 UI 任务（栈只有几 KiB）上按值构造 `Emulator`（76,288 B）、`Mapper::from_ines`（48 KiB）和 `Nrom::new`（40 KiB），叠加后栈溢出，启用即卡死。

修复：`emulator_init_in_place` 原地零初始化；`cart::Cart` 用 `Vec` 承载 PRG/CHR；UI 回调不再有大栈帧。

## 4. 卡顿根因与本次结构调整（0.1.2 时间切片）

真机反馈：不是死机，内存也没爆，手机仍能请求打开应用，只是 **UI 响应慢**；息屏再亮屏后能正常退出 → 负担全在 LVGL/UI 线程。

本次改为「模拟器离开 UI 线程」：

1. `page_on_create` 只创建控件，立即返回；ROM 加载和首帧不再同步执行。
2. 用 **Supervisor time-service 定时器** 驱动 6502 模拟，但该 pump 实际是 UI owner 上的 LVGL 定时器：改为 10 ms 跳、**每跳按墙钟时间切成 ≤4 ms 的小片**，指令预算 256–4096 自适应，跑不完留到下一跳。
3. 帧完成后用 `ctx->ui->post` 回到 UI 任务，只做 `convert_frame` + `lvx_image_set_src`；`FRAME_PENDING` 保证 UI 转换时模拟器暂停，避免读到半帧。
4. 帧内容用 FNV 校验和比较，且发布间隔 ≥50 ms：画面不变或节流窗口内不转换、不重绘。
5. `on_pause` 停止定时器、`on_resume` 重启；`on_destroy` 停止并销毁。
6. **取消 1.3125× 放大**，按 256×240 原始尺寸显示：既去掉真机上观察到的几像素边缘偏移，也砍掉最大的每帧 UI 开销。

## 5. 运行 ROM（超级玛丽）

模块只从**自己的私有目录**读 ROM（Supervisor 禁止模块用绝对路径访问 `/data/canopus`）。文件名依次尝试：

```
/data/canopus/m.nespro/nespro.nes
/data/canopus/m.nespro/supermary.nes
/data/canopus/m.nespro/rom.nes
```

都不存在时回退到内嵌的自产 NROM 测试 ROM。支持 NROM（mapper 0，含 32 KB 版本）、UxROM（2）、CNROM（3）；SMB 属于 NROM，可直接运行。

> 仓库不内置商业 ROM（版权原因），需要用户自行放入上述私有目录。

## 6. 模拟器实机验证（0.1.0 表现，0.1.1 同代码路径）

- lua 验证表盘 6/6 步通过，`state=9 (ACTIVE)`，无卡死/重启。
- launcher 应用列表最底部出现 NES 入口，点击后显示测试 ROM 的多色棋盘；右滑退出。
- 截图：`deliverables/verify/nespro-page.png`、`nespro-exit.png`，完整序列在 `/workspace/verify/nespro/`。

## 6.5 真机性能数据

页面关闭时会向模块私有目录写一次 `perf.txt`：

```
ticks=<次数> instr=<指令数> frames=<完成帧> published=<发布帧> avg_slice_ms=<平均> max_slice_ms=<最大> session_ms=<时长> instr_per_s=<吞吐> budget=<当前预算>
```

用它就能判断瓶颈是模拟吞吐还是 UI 重绘，再决定 `SLICE_TARGET_MS` / `PUBLISH_MIN_MS`。

## 7. 已知限制

1. 0.1.1 的 UI 线程改造**尚未在真机复测**（本轮按用户要求不做设备验证）。
2. 模拟器 flash journal 不可靠地承载 `registry.tmp→registry.bin` 的 rename（验证表盘才用 `os.remove`；真机 rename 原子）。
3. 仅 NROM/UxROM/CNROM；更大 ROM 受 128 KiB 模块堆限制。
4. 音频/按键/存档未实现。
