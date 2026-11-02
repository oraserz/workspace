# STATE

- 仓库/分支：`https://github.com/oraserz/workspace.git`，`trae/agent-GbKpGn`
- vendor：Canopus `4928f1eb`、x-nes `ca4c3955`（本地 symlink 到 `/workspace/.projects/*`，未修改模块管理器源码）
- 上次执行时间：2026-11-01
- 当前阶段：0.1.7（O3）+ 0.2.0 离屏基准包（`--cfg offscreen`），待用离屏数据区分核心/显示瓶颈

## 已完成

### 0.1.0（commit `efa84ee`）
- 卡死根因：`page_on_create` 在 UI 任务栈上按值构造 `Emulator`(76,288 B)+`Mapper`(48 KiB)+`Nrom`(40 KiB) → 栈溢出。
- 修复：`emulator_init_in_place` 原地零初始化 + `cart::Cart`（Vec 承载 PRG/CHR）。
- QEMU（固件 043）验证：6/6 步、`state=9 ACTIVE`、launcher 入口可打开、画面显示、右滑退出。

### 0.1.1（commit `acb6ad7`）
- 定时器驱动、`ui.post` 发布、FNV 去重、pause/resume、1:1 显示、ROM 私有目录、版本号自增。

### 0.1.2（commit `010c3ac`）
- 逆向参考 ELF 后确认：Canopus 的 time-service pump 就是 UI owner 上的 LVGL 定时器（`sup_start_timer_pump` → `CANOPUS_SUP_LV_TIMER_CREATE(...,10u,0)`），所以“移到定时器”并没有离开 UI。
- 改为 **10 ms 一跳、每次 ≤4 ms 墙钟时间片**：指令预算 256–4096 自适应（`budget*4/elapsed` 收缩，跑得完则翻倍），一帧跑不完留到下一跳。
- 发布节流 ≥50 ms；画面不变（FNV 相同）跳过转换/重绘。
- 页面关闭写一次 `perf.txt`：`ticks/instr/frames/published/avg_slice_ms/max_slice_ms/session_ms/instr_per_s/budget`。
### 0.1.3（commit `094d6be`）
- 真机反馈：0.1.2 只出现灰屏后不推进 → 固定 4 ms 片在 pump 周期变长时饿死核心。
- 改为按实测 pump 周期分配 50% 时间（3–30 ms）、预算 512–16,384 自适应。
- `FRAME_PENDING` 超过 3 跳就丢弃，避免 ui.post 丢失导致永久停摆。
### 0.1.4（commit `d8d619e`）
- 真机 perf：`ticks=519 instr=150520 frames=15 published=3 avg_slice_ms=7 max_slice_ms=16 session_ms=8295 instr_per_s=18145 budget=512`。
- 根因：每 64 条指令调用一次 `monotonic_ms`，该固件一次时钟查询≈数百条 6502 指令，7 ms 片内只跑了 ~290 条指令，自适应把预算压到 512。
- 修复：循环内不再查时钟；每个 tick 只测一次耗时，按目标 50% 占空比调整预算（512–16,384）。
### 0.1.5（commit `e60db58`）
- 0.1.4 真机 perf：`ticks=1331 instr=675023 frames=69 published=8 avg_slice_ms=16 max_slice_ms=93 session_ms=30770 instr_per_s=21937 budget=512`。
- 判定：`ctx->time->monotonic_ms` 每次约 7–8 ms，一拍两次即占满时间片；6502 解释器本身不是瓶颈。
- 改用 `CANOPUS_FW_LV_TICK_GET_CALLABLE`（`lv_tick_get`，0x1c063d0c），读全局毫秒计数。
### 0.1.6（commit `3d17e9a`）
- 用户上报的 perf 两次数值完全一致（1331/675023/69/8/16/93/30770/21937/512），判断为 0.1.4 遗留文件；0.1.5 的 LVGL tick 尚未真正测到。
- perf.txt 首行加 `ver=0.1.6 tick=lv`；页面首次 pause（息屏）且有 tick 时也写一次。
### 0.1.7（commit `34f17b3`，本次重建）
- 0.1.6 真机（LVGL tick）数据：`ticks=1289 instr=653743 frames=67 published=10 avg_slice_ms=15 max_slice_ms=92 session_ms=28209 instr_per_s=23174 budget=512` → 每指令约 30 µs，计时不是瓶颈。
- 发现 `canopus build` 强制 `CARGO_PROFILE_RELEASE_OPT_LEVEL="z"`；改用 `RUSTFLAGS="-C opt-level=3"` 覆盖。
- 版本 0.1.7（version_code 8）重建：`deliverables/nespro-0.1.7.canopus`（636,928 B，sha256 `e626af63…4337`），ELF sha256 `cf08ff10…780b`，resident 769,460 B。
- 0.2.0 离屏基准包：`deliverables/nespro-0.2.0-offscreen.canopus`（633,344 B，sha256 `3ffd8f79…6296`），ELF sha256 `3542ec6b…eb04`，resident 644,484 B；`perf.txt` 带 `mode=offscreen` 与 `ns_per_instr`。
- 风险：模块镜像从 455 KB 涨到 768 KB，若设备加载失败可退 opt-level=2。

## 未完成 / 待用户测试

1. 装 0.2.0 离屏包 → 进 NES 页面（空白页是预期）→ 等它跑完 50 万条指令自动停 → 读 `perf.txt`，看 `mode=offscreen` 与 `ns_per_instr`。
2. 测完先 REMOVE 0.2.0，再装回 0.1.7；若 `ns_per_instr` ≈ 17 µs，说明瓶颈在 x-nes 核心本身，下一步 patch x-nes（跳过 APU、减少间接分派）。
2. 观察打开/退出是否跟手；帧率是否符合预期。
3. **把 `/data/canopus/m.nespro/perf.txt` 的内容发回来**，按 `instr_per_s` / `avg_slice_ms` 调 `SLICE_TARGET_MS` 与 `PUBLISH_MIN_MS`。
4. 超级玛丽：ROM 放到 `/data/canopus/m.nespro/nespro.nes`（或 `supermary.nes`）。
