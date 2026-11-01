# STATE

- 仓库/分支：`https://github.com/oraserz/workspace.git`，`trae/agent-GbKpGn`
- vendor：Canopus `4928f1eb`、x-nes `ca4c3955`（本地 symlink 到 `/workspace/.projects/*`，未修改模块管理器源码）
- 上次执行时间：2026-11-01
- 当前阶段：0.1.0 已完成卡死修复与模拟器验证；0.1.1 针对真机 UI 卡顿做了结构性修改，**待用户真机测试**

## 已完成

### 0.1.0（已推送，commit `efa84ee`）
- 卡死根因：`page_on_create` 在 UI 任务栈上按值构造 `Emulator`(76,288 B)+`Mapper`(48 KiB)+`Nrom`(40 KiB) → 栈溢出。
- 修复：`emulator_init_in_place` 原地零初始化 + `cart::Cart`（Vec 承载 PRG/CHR）。
- QEMU（固件 043）验证：6/6 步、`state=9 ACTIVE`、launcher 入口可打开、画面显示、右滑退出。

### 0.1.1（本次）
- **模拟移出 UI 线程**：页面只建控件；模拟由 `ctx->time` 定时器任务驱动（50 ms，每跳最多 3 帧，预算 60,000 指令）。
- 帧完成后 `ctx->ui->post` 回 UI 只做 `convert_frame` + `lvx_image_set_src`；`FRAME_PENDING` 防止半帧竞争。
- FNV 校验和去重：画面不变则不转换、不重绘。
- `on_pause` 停表 / `on_resume` 启表 / `on_destroy` 销毁。
- 取消 1.3125× 放大（1:1 256×240），消除边缘偏移与主要缩放开销。
- ROM 改为模块私有目录相对路径，依次尝试 `nespro.nes` / `supermary.nes` / `rom.nes`（绝对 `/data/canopus/...` 被 Supervisor 拒绝）。
- 版本 0.1.1（version_code 2），避免同版本 `-105 STAGE_DUPLICATE` 导致旧二进制不被替换。
- 产物：`deliverables/nespro-0.1.1.canopus`（489,472 B，sha256 `b5f30bcc…4651ad`），ELF sha256 `b3ab0399…c3589f`，`package verify` OK。
- 期望画面脚本：`tools/make_expected.py`（13 色，输出 256×240 / 336×315 / 整页 PNG）。

## 未完成 / 待用户测试

1. 真机安装 0.1.1（建议先 REMOVE 旧 0.1.0，或用新版本号覆盖）：
   - 打开页面是否即时响应、退出是否顺畅；
   - 帧率/流畅度是否符合预期（当前目标 20 fps 显示、3 帧/跳 ≈ 60 Hz 游戏速度）。
2. 超级玛丽实测：把 ROM 放到 `/data/canopus/m.nespro/nespro.nes`（或 `supermary.nes`），验证 mapper 0 / 32 KB PRG 路径。
3. 若仍卡：优先怀疑 time-service 定时器回调所在任务与 UI 线程竞争，或 LVGL 重绘本身；可再降帧/降预算。
