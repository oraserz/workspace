# workspace — NES 模拟器任务基址

本仓库是「小米手环 10 Pro（043）NES 模拟器」自动化任务的**只读基址**。

## 仓库里有什么

| 路径 | 作用 |
|---|---|
| `TASKSPEC.md` | 权威任务规范（目标、必读资料、铁律、阶段、验收标准） |
| `prompts/01-research.md` | 阶段 1：调研与基座 |
| `prompts/02-implement.md` | 阶段 2：实现与上屏 |
| `prompts/03-verify.md` | 阶段 3：画面验证与收尾 |
| `prompts/04-catchup.md` | 兜底：补缺与最终交付 |
| `STATE.template.md` | 状态文件模板，执行时复制为本地 `STATE.md` |
| `resource.bin` | 小米手环 10 Pro 的 MC 游戏安装表盘，供解包分析 |

## 怎么被使用

定时任务每次执行时：

1. `git clone --depth 1 https://github.com/oraserz/workspace.git /workspace/nes-band10pro/base`，已存在则 `git pull --ff-only`；
2. 读 `base/TASKSPEC.md` 和本次阶段对应的 `base/prompts/*.md`；
3. 在 `/workspace/nes-band10pro/` 下干活，结果不回写本仓库。

## 不要放什么

- 构建产物、`.canopus` 包、ELF/中间文件；
- 任何版权 ROM、固件 bin、私钥或签名材料；
- 从 `resource.bin` 解包出的美术资源（仅本地分析用）。
