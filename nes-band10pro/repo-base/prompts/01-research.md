# 阶段 1：调研与基座

规范见 `TASKSPEC.md`。目标：把"能不能做"变成有据可依的结论与可复现的最小工程。

## 本次要做

1. 按 `TASKSPEC.md` 初始化 `/workspace/nes-band10pro/`（`STATE.md` 从 `STATE.template.md` 复制）。
2. 克隆必读仓库到 `vendor/`：Canopus、Canopus-App-LyraPlayer、x-nes、Mi8WfBinTool；`resource.bin` 已在基址仓库根目录。
3. 安装/自检工具链：`canopus doctor`；补齐 Rust 与 `thumbv8m.main-none-eabi`（soft float）目标。
4. 用 `canopus new` 建最小工程，跑通 `canopus build`、`canopus package`，记录实际命令与版本。
5. 把 `x-nes` 交叉编译到本 target（cortex-m33 / armv8-m.main / soft float），做最小链接验证（能产出静态库或可链接对象即可，不要求跑帧）。
6. 用 Mi8WfBinTool 解包 `resource.bin`（`mi8pro`），分析：图元类型、索引色/RLE 编码、图片尺寸、缩放方式、控件布局，提炼可用于 NES 帧缓冲上屏的编码/刷新思路。
7. 写 `docs/01-research.md`（以上全部结论 + 证据等级）与 `docs/DECISION.md`（模块形态、上屏路径、帧率、内存预算、ROM 策略、回退方案）。
8. 更新 `STATE.md`。

## 完成判据

- `docs/01-research.md`、`docs/DECISION.md` 存在且结论明确；
- 最小工程可 `canopus build`；
- x-nes 交叉编译成功；
- `resource.bin` 解包结果与格式分析落地。

若阶段 1 已完成，直接按 `prompts/02-implement.md` 推进。
