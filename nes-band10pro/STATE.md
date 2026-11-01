# STATE

- 仓库/分支：`https://github.com/oraserz/workspace.git`，`trae/agent-GbKpGn`
- vendor：Canopus `4928f1eb`、x-nes `ca4c3955`（本地 symlink 到 `/workspace/.projects/*`，未修改模块管理器源码）
- 上次执行时间：2026-11-01
- 当前阶段：0.1.3 = 0.1.2 的自适应占空比版 + 停摆保护，**待用户真机测试**

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
### 0.1.3（本次）
- 真机反馈：0.1.2 只出现灰屏后不推进 → 固定 4 ms 片在 pump 周期变长时饿死核心。
- 改为按实测 pump 周期分配 50% 时间（3–30 ms）、预算 512–16,384 自适应。
- `FRAME_PENDING` 超过 3 跳就丢弃，避免 ui.post 丢失导致永久停摆。
- 版本 0.1.3（version_code 4）；产物 `deliverables/nespro-0.1.3.canopus`（497,664 B，sha256 `f480b576…0714`），ELF sha256 `39b68bc5…8ac8`，`package verify` OK。

## 未完成 / 待用户测试

1. 真机安装 0.1.3（先 REMOVE 旧版本，或直接用新版本号覆盖）。
2. 观察打开/退出是否跟手；帧率是否符合预期。
3. **把 `/data/canopus/m.nespro/perf.txt` 的内容发回来**，按 `instr_per_s` / `avg_slice_ms` 调 `SLICE_TARGET_MS` 与 `PUBLISH_MIN_MS`。
4. 超级玛丽：ROM 放到 `/data/canopus/m.nespro/nespro.nes`（或 `supermary.nes`）。
