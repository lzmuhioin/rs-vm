# 子代理协作契约

适用于所有 harness（Hermes `delegate_task`、Claude Code 子代理、Codex 等）。常令见 `AGENTS.md`；本文件只讲「多个 agent 同时/接力干活」这一层。

## 1. 角色矩阵

| 角色 | 姿态 | 写 | 联网 | shell | 典型用途 |
|---|---|---|---|---|---|
| `explore` | 侦察 | 否 | 否 | 只读 | 找符号、读模块、列测试覆盖 |
| `planner` | 挑战者 | 否 | 否 | 只读 | 把计划当数据审：找遗漏、矛盾、不可验证的完成条件 |
| `reviewer` | 评审 | 否 | 否 | 只读 | 审 diff 的正确性/安全性，**不改文件、不 commit** |
| `implement` | 执行 | 是（限定范围） | 否 | 读写 | 落实现 + 测试 |
| `test` | 验证 | 是（仅测试文件） | 否 | 读写 | 补 `tests/*.rs`、跑门禁 |
| `push` | 推送 | 是 | 是 | 读写 | 只允许推特性分支；拒绝 `main`、拒绝 force-push |

权限是声明，不是默认：只读角色不给写权限；写权限必须是显式、不相交的文件范围（`exact_files` / `write_roots`）。

## 2. 派活简报（brief）

brief 只给三样东西，不要倒 transcript：

```text
objective:   一句话目标
dependencies: 已知事实（路径、符号、已跑过的命令与结果）
acceptance:  可观测的完成判据（命令 + 期望输出/退出码）
prompt:      具体要做什么；本仓约束（AGENTS.md 的哪几条相关）
```

## 3. 输出契约（五段，顺序固定）

```text
### SUMMARY   一段话：做了什么、结果如何
### EVIDENCE  path:line-range 引用 + 关键发现，每条一个 bullet
### CHANGES   改过的文件 + 一行说明；只读则字面写 "None."
### RISKS     可能出什么问题 / 父代理该复核什么
### BLOCKERS  什么挡住了；顺利则字面写 "None."
```

- `EVIDENCE` 在 `CHANGES` 之前；父代理把 `EVIDENCE` 当下一轮的 working set。
- 只读侦察角色例外：只 `SUMMARY` + `EVIDENCE`。
- 没有阻塞问题时 `planner` / `reviewer` 必须明确写「无阻塞问题」——沉默无法与「没查」区分。

## 4. 只读角色的封闭命令语法

只读 ≠ 口头保证，用同一套白名单判 shell：

- 允许：`ls` `cat` `head` `tail` `wc` `stat` `file` `du` `grep` `rg` `fd`、不带 `-exec`/`-delete` 的 `find`、`sed -n <range>p`；git 只读子命令（`status` `log` `diff` `show` `ls-files` `blame` `grep`）；`sort` `uniq` `cut` `tr` `comm`；字面 `echo`/`printf`。
- 允许 `|` `&&` `||` `;` 串联；重定向仅 `2>/dev/null`、`>/dev/null`、`2>&1`。
- 禁止：其他重定向、`$`/反引号展开、子 shell、后台、内联环境赋值、其他程序（`python` `awk` `jq` `cargo`）。
- 拒绝要可救：错误里回显规则名并给出可用替代；反复拒绝且无进展判失败，不判完成。

## 5. 并发写者纪律

- 并行编辑只许**显式不相交**的文件范围；重叠声明在准入前失败，而不是跑到一半冲突。
- 写权限三态：`read_only` / `workspace_write` / `worktree_write`。真正的隔离 worktree 才能并行写同一目录。
- 别人的活跃声明不能夺：`claim` 只加自己范围，`release` 只清已不活跃的属主。
- 读可以在写者旁边跑，但要有只读隔离；宿主没有强制手段时**运行前拒绝**，不退化成「相信它只读」。

## 6. 父代理 posture

- 子代理产出是**工作集，不是结论**；父会话保留维护者姿态，最终判断不外判。
- 合并 / 收获 / 关闭前父代理亲自看 diff、关联 issue、跑 `./scripts/check.sh`、读回改动。
- 不得仅凭标题、标签或某个代理的摘要关闭工作。

## 7. 自报 ≠ 已验证；未知 ≠ 零

- 交付物检查最多证明「存在一个非空文件」（`present` ≠ 正确 ≠ 测试通过），不能当独立质量门。
- 编辑声明按 spawn 时刻 HEAD 与脏文件内容核对：声称改了没改、改了未声明 → `claim_mismatch`。
- 没有用量、结果未知是两类**覆盖缺口**，都不按零计价；收据类产物必须带「Not recorded:」段。

## 8. 交接与恢复

- 单会话交接写**仓库外**的状态文件（分支、已完成项、下一步、已跑过的命令与读数），入库只会变噪音。
- 多 worker 用持久台账（`jsonl`）记谁欠谁一个活；恢复动词只重放台账、对账心跳，不启动新工作，且幂等。
- 长门禁用后台 + 完成通知，不用 `sleep` 轮询；退出码读日志末尾的 `rc=`，不读通知里的退出码。