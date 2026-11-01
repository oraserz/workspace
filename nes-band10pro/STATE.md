# STATE

- 依据的基址 commit：`97b12c3`（`https://github.com/oraserz/workspace.git`，分支 `trae/agent-GbKpGn`）
- vendor：Canopus、x-nes 以符号链接指向 `/workspace/.projects/*`（未修改模块管理器源码）
- 上次执行时间：2026-11-01
- 当前阶段：**模块缺陷修复 + QEMU 实机验证已完成**，产物已可交付
- 已完成：
  - **卡死根因修复**：`page_on_create` 链路原本按值构造 `Emulator`（76,288 B）+ `Mapper::from_ines`（48 KiB）+ `Nrom::new`（40 KiB），超出固件 UI 任务栈导致栈破坏/卡死。改为「原地零初始化 Emulator」+「Vec 承载 PRG/CHR 的 `cart::Cart`」，页面回调不再出现大栈帧（反汇编复核 < 512 B）
  - 最终构建：ELF sha256 `2f1c201e41d153d601eda90848fefc26a8fb591e53b65a5b3141ab7940b4a76a`，verifier `PASS (15 sections, 451,241 resident bytes, 0 undefined, 15,270 relocs, 1 ctors, 1 dtors)`
  - `.canopus` 包：487,424 B，sha256 `2b129e1dca3bbb57bb794f1bb7bf9c5bc6814e4a5110eec365a41897c9ae1510`，`canopus package verify` → `package OK`
  - **QEMU（真实固件 043）实机验证**通过：lua 验证表盘 6/6 步完成，`id=nespro state=9(ACTIVE) version=1 activation=0 modules=1 safe=0 error=0`，无卡死、无重启
  - Launcher 出现 NES 入口（应用列表最底部，罗盘图标）；点击后页面创建并持续渲染内嵌 NROM 测试 ROM 的多色棋盘画面；**右滑可退出**回到表盘
  - 探针证据（验证用临时构建，未随最终包发布）：`trace.stage1.id=204`、`trace.create/resume/frame`、`trace.frames` 60→120 持续递增、`trace.goto.ui rc=0`
  - 截图（工作区内）：`/workspace/verify/nespro/`（关键帧：`final_nes_page.png`、`final_exit.png`）
  - 交付物已更新：`deliverables/nespro-0.1.0.canopus`、`deliverables/nespro-installer-xiaomi-band-10-pro/`（.bin/.cmi/.zip）
- 已知限制：
  - 模拟器 flash journal 不保证 `registry.tmp`→`registry.bin` 的 rename，`registry.bin` 常为 0 字节；验证表盘里加了 `os.remove("/data/canopus/registry.bin")`（**仅模拟器需要**，真机 rename 原子无需此分支）
  - `ACTIVATE` 后的 stage-2 自动 `page_goto` 采用 `ctx->ui->post` 在 UI 任务执行（返回 0，页面确实创建），但在 lua 验证表盘继续操作 UI 后会被系统回收；**可靠入口是 launcher 图标**（参考实现也只保证注册与页面描述符）
  - x-nes 仅保留 NROM/UxROM/CNROM 所需 mapper；更大 ROM 仍受 128 KiB 模块堆限制
- 下次从这里继续：
  1. 若需自动打开页面，尝试在真实表盘环境（无 lua 持续 UI 操作）复测 stage-2；必要时改由模块自有 LVGL 定时器延后调用 `page_goto`
  2. 真机（非模拟器）安装 `deliverables/nespro-0.1.0.canopus` 复核；放入 `/data/canopus/nespro.nes` 验证 ROM 文件加载
