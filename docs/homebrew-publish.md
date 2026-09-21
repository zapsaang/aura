# AURA 发布流水线：Homebrew + GitHub Release 自动化

| 字段 | 值 |
|---|---|
| 状态 | 已实施 — draft v4 基线 + fork dry-run / Oracle 完整性校正；最新校正见 §19 |
| 触发 | `release.yml` 在 `v*` tag push |
| 配套 | `docs/adr.md` ADR-010；`zapsaang/homebrew-tap`（branch protection 已配置：main 要求 PR + 1 approval，admin 强制）；`zapsaang/aura` Actions secret `HOMEBREW_TAP_TOKEN` 已存在（值不暴露于任何文档） |
| 不在范围 | Homebrew bottles / macOS code signing & notarization / homebrew-core 提 PR |

---

## 1. 上下文（按事实写）

- 当前 `release.yml`（591 行）有 12 个 job：4 个 release 构建（`release-{linux,macos}-{x86,arm64}`）、`homebrew-render`、`homebrew-audit`、5 个 evidence job（`evidence-{preflight,prerequisite,ubuntu-msrv,ubuntu-gpu,home-manager-semantic}`）、1 个汇总 producer（`evidence-source`）。
- 其中 **9 个 lane**（`receipt_lane.py:16-26` 的 `LANE_JOBS`）跑 `scripts/run-compliance-lane.py --job {lane}`：`release-*`×4、`ubuntu-default`（在 `homebrew-render` job 内，`release.yml:459-476`）、`macos-default`（在 `homebrew-audit` job 内，`release.yml:524-537`）、`ubuntu-msrv`、`ubuntu-gpu`、`home-manager-semantic`。**注意**：workflow job 名与 lane 名在 homebrew 两处**不同名**——`homebrew-render` job 跑的是 `ubuntu-default` lane，`homebrew-audit` job 跑的是 `macos-default` lane。
- 3 个 producer job（`evidence-preflight`、`evidence-prerequisite`、`evidence-source`）跑 `prepare-historical-producer.py` / `prepare-source-producer.py`，**不是** lane。
- 既有 canonical tag gate：`homebrew-render` job 内（`release.yml:441-444`）已强制 `^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$` 且 `refs/tags/${TAG}^{commit} == github.sha`（用 `GIT_MASTER=1` **环境变量**形式）。**含预发布后缀的 tag（如 `v1.0.0-rc.1`、`v0.0.0-dryrun.1`）在现有流水线就会被拒**——这是既有约束，本设计沿用（见 §11.3）。
- 公式仅渲染并 `brew audit --strict` 后留在 `dist/homebrew/aura.rb`，**未**上推到任何 Homebrew tap 仓库；公式里写的 `https://github.com/zapsaang/aura/releases/download/{TAG}/...` 也**没有**对应的 GitHub Release 资产。
- `scripts/verify-publish-input.py`（76 行，`main` 在 21-49 行）在发布**前**做公式 digest + tag→commit + render/audit tuple 的交叉验证，但**输入端**到此为止，没有对应输出验证。
- 既有不属 `LANE_JOBS` 范畴的印章范式：`HandoffIdentity`/`validate_handoff_receipt`/`seal_native_handoff`（`scripts/evidence/receipt_handoff.py:1-56` + `scripts/evidence/final.py:124-162`）。**本设计复用此非 lane 印章范式**。
- `qa/compliance-qa-registry.json` 的 `count_source` 只支持 4 种值（`registry.py:166-173`）：`constant:0` / `rust-harness` / `rust-harness-sum` / `stdout-json:checks`。
- `qa/compliance-qa-registry.json` 的 `task_owner` 由 id 前缀自动派生（`registry.py:176-194` 的 `_task_owner`），不是自由字段——所以不新增 registry row/execution_context；fork 支持只更新既有 `homebrew-render` row 的 runtime binding。
- `--release src=dst` 严格只接受 `dst` 以 `releases/` 开头（`run-compliance-lane.py:84-88`）；现有 `RELEASE_PATHS` 已含 `releases/homebrew/aura.rb`（`scripts/evidence/receipt_aggregate.py:18-24`）。
- render/audit 的 tuple 目录只存在于各自 job 的 workspace，并被打包进**内容寻址名**的 lane archive artifact（`aura-lane-<lane>-<sha256>`）；publish job 无法按名下载。**因此 render/audit job 需追加固定名 tuple artifact 上传**（见 §3 表注与 §6）。

## 2. 目标与非目标

**目标**

- 同 tag push 后自动：上传 4 个 `aura-*.tar.gz` + 4 个 `aura-*.tar.gz.sha256` + **`aura.rb`**（共 9 个资产）到当前 `${{ github.repository }}` 的 draft GitHub Release；正式仓默认仍为 `zapsaang/aura`。
- 自动在 `zapsaang/homebrew-tap` 仓库开 PR 修改 `Formula/aura.rb`，让维护者 review+merge。
- 副作用均经过 evidence-bound 框架封印成可审计 receipt；不另起新模式。
- 同 tag 重跑严格幂等；同 tag 并发被串行化。

**非目标（v1）**

- Homebrew bottles（prebuilt cctools-relocation）—— 加 `sonoma`/`arm64_sequoia` runner，复杂度远超 v1。
- macOS code signing & notarization —— 需要 Apple Developer ID 凭据，单独 PR。
- homebrew-core 提 PR、`brew livecheck` 集成、AUR/Scoop 同步。
- 自动合并 tap PR（维护者必须人工 OK）。
- Release draft → published 的翻转 —— **v1 无 follow-up workflow**，由维护者在合并 tap PR 后手工执行 `gh release edit $TAG --draft=false`（见 §6.1.1）。

## 3. 不动的部分（已事实存在，仅复用）

| 资产 | 路径 | 现有行为 |
|---|---|---|
| 4 平台 release 构建 | `release.yml`（各 `release-*` job） | 跑 `run-compliance-lane.py --job release-{linux,macos}-{x86,arm64}`；产物 `aura-*.tar.gz`+`.sha256`，artifact 名 `aura-<target>`（`release.yml:53,107,160,213`） |
| 公式渲染 | `release.yml` `homebrew-render` job (ubuntu) | 跑 `ubuntu-default` lane，内含 `scripts/render-homebrew-formula.py`（91 行），输出 `dist/homebrew/aura.rb` + `.sha256`；artifact 名 `homebrew-formula`（`release.yml:484`） |
| 公式 audit | `release.yml` `homebrew-audit` job (macos-15) | 跑 `macos-default` lane，内含 `brew tap-new aura/audit && brew audit --strict --formula aura/audit/aura` |
| 发布前 gate | `scripts/verify-publish-input.py:21-49` | 校验 `tag==vMAJOR.MINOR.PATCH`、`refs/tags/{TAG}^{commit}==VERIFIED_COMMIT`、公式 digest、4 个 `download/{TAG}/...` 全部嵌入 |
| 公式模板 | `deployment/homebrew/aura.rb.in:1-43` | 4 个 `url/sha256 on_{macos,linux} on_{arm,intel}` 块；release URL 的仓库由 `{SOURCE_REPOSITORY}` 提供，默认 `zapsaang/aura`，另有 `{TAG}` 与 4 个 `{SHA256_*}` 占位符 |
| 哈希打包 | `scripts/package-release.py:1-96` | 决定论 tarball，成员固定 `{aura-cli, aura-daemon, SHA256SUMS}`（SHA256SUMS 在 tarball **内部**，不作为独立 release 资产） |
| 印章范式 | `scripts/seal-native-handoff.py:1-46` | non-lane seal 范例，`scripts/evidence/final.py:seal_native_handoff` 写 `HandoffIdentity` receipt |

> **表注（新增，非"不动"）**：`homebrew-render` / `homebrew-audit` 两 job 需各追加一个 `actions/upload-artifact` step，把 tuple 目录以**固定名**上传：
> - render job：路径 `.omo/evidence/design-compliance-remediation/lane/ubuntu-default/tuples/homebrew-render`，artifact 名 `homebrew-render-tuple`
> - audit job：路径 `.omo/evidence/design-compliance-remediation/lane/macos-default/tuples/homebrew-audit`，artifact 名 `homebrew-audit-tuple`
>
> 这是 publish job 能重跑 `verify-publish-input.py` 的唯一数据来源（lane archive 名是内容寻址的，无法按名下载）。

## 4. 架构（保留现状，追加 publish 两段）

```
                tag v* push  ─┐
                                │
   ┌───────────┬───────────────┼─────────────────────────┐
   │           │               │                         │
 rel-         rel-           rel-                      rel-
 linux-x86    linux-arm64    macos-arm64               macos-x86
 (4× ubuntu / macos native runner)
   │           │               │                         │
   └─────┬─────┴───────┬───────┴───────────┬─────────────┘
         │             │                   │
  homebrew-render (ubuntu)        homebrew-audit (macos-15)
  [ubuntu-default lane]           [macos-default lane]
  + 上传 homebrew-render-tuple    + 上传 homebrew-audit-tuple
         │             │                   │
         └──────┬──────┴───────────────────┘
                │
   ★ publish-github-release  (ubuntu-24.04, no lane)
       1. download 4 release-* artifacts + homebrew-formula + 2 tuple artifacts
       2. re-verify tag/commit binding (GIT_MASTER=1 env 形式)
       3. 重跑 verify-publish-input.py（tuple 已就位）
       4. shasum → 4 SHA256 + formula digest
       5. 本地渲染 release notes（不查询 gh，避免鸡生蛋）
       6. gh release view $TAG → 存在? upload --clobber（9 资产）
                                : create --draft（9 资产）
        7. GitHub API 精确核对远端 9 个资产的名字+digest，并从 API 响应写 manifest
        8. seal 前复算复制后的 manifest digest，再写 publish/github-release/receipt.json（留 draft）
                │
   ★ publish-homebrew  (ubuntu-24.04, no lane)
       1. download 同上全部 artifacts
       2. re-verify tag/commit + verify-publish-input
       3. verify-release-asset（GitHub API 核对 asset digest，支持 draft）
       4. credential helper → git clone homebrew-tap
       5. detect default branch + reuse existing PR if any
       6. 内容比对公式 → cp + commit + force-with-lease push（或 no-op）
       7. render-publish-pr-body → pr-body.md
       8. gh pr create / edit
       9. seal publish/homebrew/receipt.json（no-op 也封印）
                ▼
       维护者审 PR → merge tap
       维护者手工翻转 release（v1 唯一方式）:
        gh release edit $TAG --repo "$GITHUB_REPOSITORY" --draft=false
       用户 `brew upgrade aura` 即生效
```

依赖：`publish-github-release` 只依赖 4 个 release-* + 2 个 homebrew-*。`publish-homebrew` 同时依赖 publish-github-release（验证 release 真存在且资产 digest 正确）。draft → published 的最后一步**不在任何 workflow 里**：v1 由维护者在 tap PR merge 后手工执行带 source `--repo` 的 `gh release edit`。

## 5. registry / execution_context 的最小改动

不在 `qa/compliance-qa-registry.json` 新增 entry、不在 `LANE_JOBS`/`LANE_CONTRACTS`/`_validate_tools` 加 lane、不扩 `test_workflow_contract.py` 的 jobs 列表。唯一 registry 改动是既有 `homebrew-render` 行增加 `SOURCE_REPOSITORY=<github-repository>` runtime binding，并把它传给 `render-homebrew-formula.py --source-repository`；row 数、lane 数、count source 与 execution context 全部不变。

**决策原因**

- `count_source` 只支持 4 类（`registry.py:166-173`） + `task_owner` 自动派生（`registry.py:176-194`）；publish 是副作用不是 verification，进 registry 会引入"`constant:0` 不验证副作用"的逻辑漏洞。
- `test_workflow_contract.py:test_release_workflow_produces_every_lane_without_assembling` 硬编码 9 个 lane；每个新 entry 都要 `--job X --archive X` 召唤 lane，与"副作用不该走 lane"原则冲突。
- **关键**：现有 `scripts/run-evidence-command.py:39-52` 在每个 registry 命令前后做 `worktree_state` diff 检查，子命令要求 `clean checkout`、`registry command mutated worktree state` 直接 raise `EvidenceError`。我们的 publish step 要 `git clone tap`、`cp formula`、`commit`、`push`、`write_manifest` —— 这全是工作树变更，**根本不能进 registry**。这一条是技术阻断，不只是风格选择。
- 现有 `seal-native-handoff.py` 已示范 non-lane seal（`scripts/evidence/final.py:124-162`）是合规的范式 —— 沿用即可。

**唯一允许的间接 hook**：在既有 `homebrew-render` tuple 中封存 source repository binding，并在 `verify-publish-input.py` 的 audit-tuple / render-tuple 路径里复验；不新增 schema 或 lane。

> 进入 publish 步骤的 shell 脚本**必须**加 `set -euo pipefail`，并在 step 顶层 `shell:` 行写 `bash --noprofile --norc -eo pipefail {0}`。这与 registry 的 `CommandSpec.shell` 经 `shlex.split` 后以 `-c` 执行命令的 hardening 思路类同（`run-evidence-command.py:42-49`）。这是发布副作用脚本的标准 hardening。

## 6. 新增 step-by-step 规格

### 6.1 `publish-github-release` job

| step | 行为 | 失败处理 |
|---|---|---|
| 1. checkout at ${{ github.sha }}，`fetch-depth: 0` | 必须；`refs/tags/${TAG}^{commit}` 需要历史 | exit 1 |
| 2. 无需 setup-rust / setup-python | ubuntu-24.04 runner 自带 python3.12；publish 不构建 | — |
| 3. download 4 个 release artifact：`aura-x86_64-unknown-linux-gnu`、`aura-aarch64-unknown-linux-gnu`、`aura-x86_64-apple-darwin`、`aura-aarch64-apple-darwin`（上传名见 `release.yml:53,107,160,213`）到 `artifacts/`。**每个 artifact 用独立 `actions/download-artifact` step（`name:` + `path: artifacts/<name>`）；禁止无 `name` 的全量下载**——届时仓内已有 12+ 个内容寻址 lane/producer archive，全量下载会把它们全部拉入 publish job | — | file missing 则 exit |
| 4. download `homebrew-formula` artifact（`release.yml:484`）到 `dist/homebrew/` | — | file missing 则 exit |
| 5. download `homebrew-render-tuple` 到 `.omo/evidence/design-compliance-remediation/lane/ubuntu-default/tuples/homebrew-render`；download `homebrew-audit-tuple` 到 `.omo/evidence/design-compliance-remediation/lane/macos-default/tuples/homebrew-audit` | 路径与上游 job 内一致，`verify-publish-input.py` 不用改 | file missing 则 exit |
| 6. `echo "$TAG" \| grep -qE '^v(0\|[1-9][0-9]*)\.(0\|[1-9][0-9]*)\.(0\|[1-9][0-9]*)$'` | 与既有 gate（`release.yml:443`、`model.py:12` 的 `TAG` regex）一致 | exit 1 |
| 7. `test "$(GIT_MASTER=1 git rev-parse --verify "refs/tags/${TAG}^{commit}")" = "${{ github.sha }}"` | **env 形式**（与 `release.yml:444`、`verify-publish-input.py:63` 一致）；`git -c GIT_MASTER=1` 是非法 config 语法，禁用 | exit 1 |
| 8. 跑 `scripts/verify-publish-input.py --formula dist/homebrew/aura.rb --sha256 "$(cat dist/homebrew/aura.rb.sha256)" --audit-tuple .omo/evidence/design-compliance-remediation/lane/macos-default/tuples/homebrew-audit --render-tuple .omo/evidence/design-compliance-remediation/lane/ubuntu-default/tuples/homebrew-render --tag "$TAG" --verified-commit "${{ github.sha }}"` | audit tuple 在 **macos-default** lane 下（不是 `lane/homebrew-audit`）；render tuple 在 **ubuntu-default** lane 下 | exit 1 → 整 job 红 |
| 9. 计算 4 个 tarball SHA256 入 env `SHA_LINUX_X86` / `SHA_LINUX_ARM` / `SHA_MACOS_X86` / `SHA_MACOS_ARM` | 与 `homebrew-render` job 内变量名一致（`release.yml:455-458`） | missing file → exit 1 |
| 10. 渲染 release notes（§6.1.2，**纯本地**，不调 `gh release view`） | — | exit 1 |
| 11. release 不存在时 `gh release create --draft --title --notes-file` 并上传 9 资产；已存在且仍为 draft 时先 `gh release edit --title --notes-file`，再 `gh release upload --clobber` 全部 9 资产 | 重跑同步更新标题、release notes 与资产；已 published 仍拒绝修改 | 错误透传到 stderr |
| 12. 从本地 9 个待上传文件生成 expected manifest；随后 `verify-release-asset.py` 用 GitHub API 对远端**精确名字集合与全部 9 个 digest**逐项核对 | 同数量但错名、任一 `.sha256`/formula/tarball digest 漂移均 exit 1 | exit 1 |
| 13. 此时**留为 draft**——发布翻转不在本 job。详见 §6.1.1 | — | — |
| 14. `verify-release-asset.py --assets-manifest-out "$SEAL_ROOT/assets.txt"` 从已认证 GitHub API 响应写 canonical 9 行 manifest；再计算 `--assets-manifest-sha256`，并捕获 `RELEASE_URL`/`RELEASE_ID` | manifest 不是本地推测值，而是远端事实；文件缺 → exit 1 | — |
| 15. `seal-publish.py --target github-release ... --assets-manifest-sha256 "$SHA"`；`seal_publish` 对 `$SEAL_ROOT/assets.txt` 复算 digest，必须与调用方参数相同后才 seal | 防止调用方传入与复制 manifest 无关的 digest | exit 1 |
| 16. `id: seal-digest`，`shasum -a 256 .omo/evidence/design-compliance-remediation/publish/github-release/receipt.json` → `echo "sha256=..." >> "$GITHUB_OUTPUT"` | 与 lane job 的 `lane-digest` step 同范式（`release.yml:58-60`） | — |
| 17. `actions/upload-artifact@v4`（pin `ea165f8d65b6e75b540449e92b4886f43607fa02`，与全仓一致）以 `aura-publish-github-release-${{ steps.seal-digest.outputs.sha256 }}` 上传 `receipt.json` + `SHA256SUMS` | 内容寻址名，与 lane archive 同范式 | if-no-files-found: error |

#### 6.1.1 draft vs published 时机

**争议来源**：公式 PR 可能被拒绝 / 长时间挂起。此时 GitHub Release 若已 published，asset URL 就被 brew 拉走。

**v1 决策（已按实施复核）**：Release 永远 `draft`；`publish` 这步**不在** `publish-github-release` 里，也**不存在** follow-up workflow。合并 tap PR 后由维护者手工执行 `gh release edit $TAG --draft=false`。v1 没有 `on: pull_request closed` 触发器；将来若加自动翻转，是独立的后续 PR，不在本设计内。

**简化的 v1 替代（即实施现状）**：`gh release create --draft` 完结；维护者合并 PR 后手工 `gh release edit $TAG --draft=false`。

> **发布后不可变（实施事实）**：workflow 重跑时对已 published 的 release **显式拒绝变更**——`gh release view $TAG --json isDraft` 返回非 `true` 即 `exit 1`（"Refusing to mutate published release"）。即 published release 对 workflow 重跑是刻意不可变的：同 tag 二刷只在 release 仍为 draft 时 `--clobber` 资产。

> **事实校正（第 2 轮）**：draft release 的资产 URL 对**匿名请求返回 404**——`releases/download/...` 只在 release published 后公开可达。因此：(a) 任何"用未认证 HTTP 探活 asset URL"的设计都会在 draft 阶段必然失败；(b) §7.2 改用**带 token 的 GitHub API** 核对资产 digest；(c) brew 用户在 PR 合并 + release 翻转前 `brew install` 必然 404，PR body checklist 里的安装验证只能发生在翻转之后。

#### 6.1.2 release notes 模板

```bash
RELEASE_NOTES="$RUNNER_TEMP/release-notes-${TAG}.md"
cat > "$RELEASE_NOTES" <<EOF
# Aura ${TAG}

Verified commit: \`${GITHUB_SHA}\`

## SHA-256

| asset | sha256 |
|---|---|
| aura-x86_64-unknown-linux-gnu.tar.gz | ${SHA_LINUX_X86} |
| aura-aarch64-unknown-linux-gnu.tar.gz | ${SHA_LINUX_ARM} |
| aura-x86_64-apple-darwin.tar.gz | ${SHA_MACOS_X86} |
| aura-aarch64-apple-darwin.tar.gz | ${SHA_MACOS_ARM} |

## Audit log
https://github.com/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}

## Homebrew
\`brew install zapsaang/tap/aura\`（tap PR 合并且 release 翻转后）
EOF
```

**确定性**：notes 全部来自本地 env（step 9 的 SHA 与 `GITHUB_*`），**不在 create 前调 `gh release view`**——首跑时 release 尚不存在，查询会鸡生蛋失败。SHA256SUMS 不作为独立资产（它内嵌在 tarball 里，见 §3），notes 改为内联 digest 表。

### 6.2 `publish-homebrew` job

| step | 行为 | 失败处理 |
|---|---|---|
| 1-5 | 同 6.1 步骤 1-5（重新下载 artifacts，含 2 个 tuple artifact） | — |
| 6 | tag 规范化检查（同 6.1 step 6） | exit 1 |
| 7 | `GIT_MASTER=1 git rev-parse --verify "refs/tags/${TAG}^{commit}"` 等于 `${{ github.sha }}`（env 形式，同 6.1 step 7） | exit 1 |
| 8 | 重跑 `scripts/verify-publish-input.py`（**防御性**，input 状态可能因 publish-github-release 中途改了 artifact 而漂移；参数同 6.1 step 8） | exit 1 |
| 9 | 从下载的本地 9 资产生成 expected manifest；跑 `scripts/verify-release-asset.py --formula dist/homebrew/aura.rb --tag "$TAG" --repo "${{ github.repository }}" --expected-manifest ... --assets-manifest-out ...`，env 注入 `AURA_RELEASE_TOKEN=${{ secrets.GITHUB_TOKEN }}`（见 §7.2） | exit 1 |
| 10 | env block：`HOMEBREW_TAP_TOKEN`、`GH_TOKEN` 两者都注入 `secrets.HOMEBREW_TAP_TOKEN` | env miss → exit 1 |
| 11 | 见下方"credential helper script"块注入 token（避免 token 进 `~/.gitconfig`） | exit 1 |
| 12 | 探 tap 默认 branch：`git -C tap symbolic-ref refs/remotes/origin/HEAD \| sed 's@^refs/remotes/origin/@@'` → `TAP_DEFAULT`，避免硬编 `main` | exit 1 |
| 13 | `gh pr list` 查询 `number,headRefName,headRepository,isCrossRepository`；仅当分支/base 匹配、`headRepository.nameWithOwner == TAP_REPOSITORY` 且 `isCrossRepository == false` 时复用。fork/cross-repository 同名分支 PR 必须拒绝，避免碰撞 | exit 1 if `gh` fail or candidate ownership is unsafe |
| 14 | if 同 PR 已存在：`cd tap && git fetch origin pull/<N>/head:aura-${TAG} && git checkout aura-${TAG}`；else `git checkout -b aura-${TAG} "origin/${TAP_DEFAULT}"` | exit 1 |
| 15 | no-op 判定：**直接内容比对** `if cmp -s dist/homebrew/aura.rb tap/Formula/aura.rb; then STATUS=no-op; else STATUS=approved; fi`——比较的是新渲染公式与 tap 当前公式的内容，不依赖 git ref diff | — |
| 16 | `cp dist/homebrew/aura.rb tap/Formula/aura.rb`（仅 STATUS=approved） | exit 1 |
| 17 | `git -C tap add Formula/aura.rb && git -C tap -c user.name="aura-publisher[bot]" -c user.email="${BOT_EMAIL:-<bot-id>+aura-publisher@users.noreply.github.com}" commit -qm "aura ${TAG}"`（仅 STATUS=approved） | exit 1 |
| 18 | `REMOTE_SHA="$(git -C tap rev-parse "refs/remotes/origin/aura-${TAG}" 2>/dev/null \|\| echo 0000000000000000000000000000000000000000)"`<br>`git -C tap push --force-with-lease=refs/heads/aura-${TAG}:$REMOTE_SHA origin aura-${TAG}`（仅 STATUS=approved） | exit 1 |
| 19 | 写出 4 个 formula tarball digest 后运行 `render-publish-pr-body.py ... --release-url "$RELEASE_URL" --audit-log-url "$AUDIT_LOG_URL" --tap-repository "$TAP_REPOSITORY"`。脚本从 release URL 得出 source repo，从 configured tap repo 得出 Homebrew tap 名 | exit 1 |
| 20 | if `EXISTING_PR` 存在：`gh pr edit "$EXISTING_PR" --body-file tap/.pr-body.md`；else：`gh pr create --repo zapsaang/homebrew-tap --base "$TAP_DEFAULT" --head aura-${TAG} --title "aura ${TAG}" --body-file tap/.pr-body.md`（仅 STATUS=approved）。**随后统一取号**：`gh pr view "aura-${TAG}" --repo zapsaang/homebrew-tap --json number,url` → `PR_NUMBER`/`PR_URL`（create/edit 两路径同源，step 21 的 `--pr-number`/`--pr-url` 数据来源，此前文档未声明） | exit 1 |
| 21 | `python3 scripts/seal-publish.py --target homebrew --pr-number "${PR_NUMBER:-0}" --pr-branch "${PR_BRANCH:-}" --pr-url "${PR_URL:-}" --tap-default-branch "$TAP_DEFAULT" --status "$STATUS" ...`（见 §7.1；no-op 且无 PR 时 pr-number=0、pr-url/pr-branch 为空串） | exit 1 |
| 22 | `id: seal-digest`，`shasum -a 256 .omo/evidence/design-compliance-remediation/publish/homebrew/receipt.json` → `echo "sha256=..." >> "$GITHUB_OUTPUT"` | 同 6.1 step 16 | — |
| 23 | `actions/upload-artifact@v4`（pin 同 6.1 step 17）以 `aura-publish-homebrew-${{ steps.seal-digest.outputs.sha256 }}` 上传 `receipt.json` + `SHA256SUMS` | 内容寻址名 | if-no-files-found: error |

#### 6.2.1 step 11 credential helper script（完整）

```bash
# 用 set -euo pipefail (此 step 内单独 shell 块保持 hardening)
ASKPASS="$RUNNER_TEMP/git-askpass.sh"
cat > "$ASKPASS" <<'EOF'
#!/bin/sh
# GIT_ASKPASS 协议：git 以提示文本为 $1 调用本脚本，脚本必须只输出凭据本身。
case "$1" in
  *sername*) printf '%s\n' "x-access-token" ;;
  *) printf '%s\n' "$HOMEBREW_TAP_TOKEN" ;;
esac
EOF
chmod 700 "$ASKPASS"
GIT_ASKPASS="$ASKPASS" git -c credential.helper= -c core.askPass="$ASKPASS" \
  clone --depth 1 https://github.com/zapsaang/homebrew-tap.git tap
```

> **关键**：heredoc 用 `'EOF'`（quoted）让 `$HOMEBREW_TAP_TOKEN` 在**写盘时**不被展开——token 值永不落盘，脚本被 git 调用时从进程环境读取。`chmod 700` 兜底。step 10 env block 把 `HOMEBREW_TAP_TOKEN` 注入 step 11 的 shell 环境，git 子进程继承。
>
> **askpass 输出格式**：git 对每个提示分别调用脚本（Username 一次、Password 一次），脚本 stdout 的**第一行**就是凭据。输出 `username=...`/`password=...` 两行会被当成字面用户名，认证必败——必须按 `$1` 分支只回值。

> **避免 `:prompt` 截留**：`git -c credential.helper=` 把默认 helper 清空（防止 GH Actions runner 默认 store），然后 `core.askPass` 指向脚本。

> **bot identity**：实施使用固定 noreply 邮箱 `41898282+aura-publisher[bot]@users.noreply.github.com` 以 `aura-publisher[bot]` 名义提交；GitHub Actions 推送者身份，无需单独 bot 账号，也不需要在 `homebrew-tap` 加 collaborator——branch protection（PR + 1 approval，admin 强制）由维护者账号 review 满足。
> **no-op 仍封印**：即使 step 15 判定公式未变，step 21 仍然调 `seal-publish.py` 写 `status: "no-op"` receipt —— 让运维从 `aura-publish-homebrew-${sha}` artifact 里清楚看到"此轮没做事"。无 PR 的 no-op（公式与 tap 默认分支一致且无打开 PR）以 `pr_number: 0` + 空 `pr_url`/`pr_branch` 封印，validator 对此组合显式放行（见 §8.2）。
> **PR 复用与 title 同源**：step 13 按分支 `aura-${TAG}` + base 匹配既有 PR；step 20 的 PR title 固定为 `aura ${TAG}`。分支名派生自 tag，天然与 title 同步；改 PR title 不影响复用判定。
> **commit vs PR title 区分**：step 17 commit message 是 `aura ${TAG}`（提交历史 grep 友好）；step 20 PR title 也是 `aura ${TAG}`（PR list 里 appear 一致）。两者解耦：可任意改 PR title 而不动 commit，反之亦然。

## 7. 新增脚本契约

> 所有新脚本遵循既有 `scripts/{NAME}.py` 模式：shebang `#!/usr/bin/env python3`、`from __future__ import annotations`、`sys.dont_write_bytecode = True`、在 `__main__` 末尾 `raise SystemExit(main())` 包 `(EvidenceError, OSError)`。

### 7.1 `scripts/seal-publish.py`（~80 LOC）

**职责**：写 `publish-{target}.receipt.json`。

**入参**（按 target 不同）：
- `--root DIR`
- `--target {github-release,homebrew}`
- `--run-id`, `--run-attempt`（来自 `github.run_id`/`run_attempt`）
- `--tag`, `--verified-commit`
- `--formula-sha256 HEX` —— 64hex 字符串（调用方 `cat dist/homebrew/aura.rb.sha256` 而得；脚本内 `require_hex(..., 64, ...)` 校验，**不**接受路径，保持与 `seal-native-handoff.py` 的纯值入参风格一致）
- `--status {approved,no-op}`（默认 `approved`）
- target=github-release：`--release-url URL`, `--release-id INT`, `--assets-manifest-sha256 HEX`（SHA of `assets.txt`）
- target=homebrew：`--pr-number INT`（no-op 无 PR 时传 0）, `--pr-branch NAME`（可空）, `--pr-url URL`（可空）, `--tap-default-branch NAME`

**输出**：`{root}/receipt.json` schema（与 §8.2 COMMON_KEYS ∪ {target extras} 一致）。

**调用链**：
- 入口：`seal-publish.py` 解析 CLI → 构造 `PublishIdentity(target, ...)` → 调 `evidence.final.seal_publish(root, identity, extras)`
- `evidence.final.seal_publish`：先 build 临时 `extras` 校验，再 `validate_publish_receipt`，写 receipt.json（manifest 语义见 §8.3）
- 与 `seal-native-handoff.py` 风格完全同源

**契约**：
- 用既有 `evidence.model` 的 `require_*` / `canonical_json_bytes` / `write_new` helpers
- 不重复 isinstance 链

### 7.2 `scripts/verify-release-asset.py`（~60 LOC）

**职责**：用 GitHub API 核对 release 的精确 9 资产集合、全部 9 个 digest，以及公式里的 4 个 `url`/`sha256` 对。**不用未认证 HTTP 探活 URL**——draft release 的资产 URL 对匿名请求 404。

**入参**：
- `--formula PATH`
- `--tag TAG`（canonical tag；与公式 URL 里的 `/download/{TAG}/` 段交叉比对）
- `--repo OWNER/NAME`（default `zapsaang/aura`）
- `--expected-manifest PATH`（本地待上传 9 资产的 canonical `<name> <sha256>` 清单）
- `--assets-manifest-out PATH`（成功后从 GitHub API 响应写出的 canonical 远端清单）
- `--timeout SECONDS`（default 10）

**环境**：`AURA_RELEASE_TOKEN`（必填；publish-homebrew step 9 注入 `secrets.GITHUB_TOKEN`，本仓 `contents:read` 即可读 draft release API；**不**复用 `GH_TOKEN`，因为该 env 在 publish-homebrew 里是 scoped 到 homebrew-tap 的 PAT，调本仓 API 会 401）。

**行为**：
- 离线解析公式：`re.findall(r'url "([^"]+)"\s*\n\s*sha256 "([0-9a-f]{64})"', formula_text)`，必须恰好 4 对，且每个 URL 含 `/download/{tag}/`。
- `urllib.request.Request(f"https://api.github.com/repos/{repo}/releases/tags/{tag}", headers={"Authorization": "Bearer ...", "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"})` → `urlopen(req, timeout=...)`。
- 校验：(a) 远端名字集合必须精确等于 `aura.rb` + 4 tarball + 4 `.sha256`；(b) 9 个远端 `digest` 全部等于 expected manifest；(c) 公式 4 个 digest 同时等于 expected/remote tarball digest。
- `--assets-manifest-out` 的字节只来自已验证的 GitHub API 响应，按名字排序；不能把本地 expected manifest 直接复制成 receipt 证据。
- token 只进请求头，绝不进 stdout/stderr/日志。
- **可测性契约（TDD）**：以 `import urllib.request` + `urllib.request.urlopen(req, timeout=...)` 形式调用，**不得** `from urllib.request import urlopen`——单测的 patch 目标固定为 `urllib.request.urlopen`（§11.1 用例 1/2/7 的 mock 点）。

**输出契约（与既有脚本一致的 stdout-json 风格）**：
- 成功：stdout `{"checks":10,"status":"ok"}\n`（1 个 exact-set closure + 9 个 digest），stderr 为空
- 失败：stderr `verify-release-asset: <reason>\n`（reason 不含 token），exit 1

### 7.3 `scripts/render-publish-pr-body.py`（~80 LOC）

**职责**：拼 PR markdown 给维护者 review。

**入参**：
- `--tag`, `--verified-commit`
- `--formula-sha256`（64hex）
- `--asset-sha-list PATH`（4 行：`aura-x86_64-unknown-linux-gnu.tar.gz <sha>` 等；可由 `for t in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin; do shasum -a 256 artifacts/aura-$t/aura-$t.tar.gz; done` 在前面 step 写出）
- `--release-url`
- `--audit-log-url`（GitHub Actions UI URL of `homebrew-audit` job。publish-homebrew 从 `repos/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}/jobs` 获取；查不到时降级为同一 source repository 的 run 级 URL，不允许留空）
- `--tap-repository OWNER/homebrew-NAME`（default `zapsaang/homebrew-tap`；渲染成 `brew tap OWNER/NAME`）
- `--out PATH`

**输出**：
- markdown 含：
  - 表格：tag / commit / 4 target + url + 期望 sha256
  - 链接：release / audit / release.yml run
  - checklist（**顺序反映 draft 现实**：翻转前 brew install 必然 404）：
    - [ ] 维护者确认 `gh release view $TAG --repo SOURCE_OWNER/SOURCE_REPO --json assets` 资产数 = 9（draft 对协作者可见）
    - [ ] 合并本 PR
    - [ ] 翻转 release：`gh release edit $TAG --repo SOURCE_OWNER/SOURCE_REPO --draft=false`
    - [ ] 翻转后验证：`brew tap TAP_OWNER/TAP_NAME && brew install aura`；生产默认仍渲染 `zapsaang/tap`

**契约**：与 `scripts/verify-publish-input.py:21-49` 风格同源；输出本身没 `count_source`（它是被 `python3 step` 调起，不是 registry entry）。

## 8. evidence-bound 框架变更（**只剩这些**）

> 与既有 `HandoffIdentity`/`validate_handoff_receipt`/`seal_native_handoff`（`scripts/evidence/receipt_handoff.py:1-56` + `scripts/evidence/final.py:124-162`）同级。新增最少。

### 8.1 拆模块：**receipt_publish.py 放 identity + validate；final.py 放 seal_publish**

**原因**：`scripts/evidence/final.py:124` 的 `seal_native_handoff` 是 precedent；新 seal 写在同文件，让 "non-lane seal" 接口集中。身份与校验放 `receipt_publish.py`，与 `receipt_handoff.py` 同级。

### 8.2 `scripts/evidence/receipt_publish.py`（新文件，~90 LOC）

```python
from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

from .model import (
    fail, require_enum, require_exact_keys, require_hex,
    require_safe_id, require_string, require_tag, require_uint,
)
from .receipt_common import _schema_status


@dataclass(frozen=True)
class PublishIdentity:
    target: Literal["github-release", "homebrew"]
    run_id: str
    run_attempt: int
    tag: str
    verified_commit: str
    formula_sha256: str  # 锚定 dist/homebrew/aura.rb digest


# Common keys present in EVERY publish receipt.
COMMON_KEYS = frozenset({
    "schema_version", "status", "target", "run_id", "run_attempt",
    "tag", "verified_commit", "formula_sha256", "manifest_sha256",
    "created_at", "seal_actor",
})
# Target-specific extras (added to COMMON_KEYS for require_exact_keys).
GITHUB_RELEASE_EXTRA = frozenset({"release_url", "release_id", "assets_manifest_sha256"})
HOMEBREW_EXTRA = frozenset({"pr_number", "pr_branch", "pr_url", "tap_default_branch"})

ALLOWED_STATUS = frozenset({"approved", "no-op"})


def validate_publish_receipt(payload: dict[str, object], identity: PublishIdentity) -> None:
    """target-specific key set; shared base keys; tie identity → receipt."""
    extras = GITHUB_RELEASE_EXTRA if identity.target == "github-release" else HOMEBREW_EXTRA
    require_exact_keys(payload, COMMON_KEYS | extras, f"{identity.target} publish receipt")
    # schema_version only; status checked separately (no-op allowed).
    _schema_status(payload, f"{identity.target} publish receipt", status=False)
    status = require_enum(payload["status"], ALLOWED_STATUS, "publish status")
    if payload["target"] != identity.target:
        fail("publish target drift")
    if require_safe_id(payload["run_id"], "publish run_id") != identity.run_id:
        fail("publish run_id drift")
    if require_uint(payload["run_attempt"], "publish run_attempt", positive=True) != identity.run_attempt:
        fail("publish run_attempt drift")
    if require_tag(payload["tag"]) != identity.tag:
        fail("publish tag drift")
    if require_hex(payload["verified_commit"], 40, "publish verified_commit") != identity.verified_commit:
        fail("publish verified_commit drift")
    if require_hex(payload["formula_sha256"], 64, "publish formula_sha256") != identity.formula_sha256:
        fail("publish formula_sha256 drift")
    require_hex(payload["manifest_sha256"], 64, "publish manifest_sha256")
    require_string(payload["created_at"], "publish created_at")
    if payload["seal_actor"] != "aura-publisher":
        fail("publish seal_actor drift")
    if identity.target == "github-release":
        require_string(payload["release_url"], "publish release_url")
        require_uint(payload["release_id"], "publish release_id", positive=True)
        require_hex(payload["assets_manifest_sha256"], 64,
                    "publish assets_manifest_sha256")
    else:
        pr_number = require_uint(payload["pr_number"], "publish pr_number")
        pr_branch = require_string(payload["pr_branch"], "publish pr_branch")
        pr_url = require_string(payload["pr_url"], "publish pr_url")
        require_string(payload["tap_default_branch"], "publish tap_default_branch")
        if status == "no-op" and pr_number == 0:
            # 公式与 tap 默认分支一致且无打开 PR：无可引用 PR，显式允许空三元组。
            if pr_branch or pr_url:
                fail("no-op without PR must have empty pr_branch/pr_url")
        else:
            if pr_number < 1 or not pr_branch or not pr_url:
                fail("publish PR fields incomplete")
```

> 注：避开 `_schema_status(..., status=True)`，因为 `status=True` **强制 `approved`**，与 no-op 路径冲突（`receipt_common.py:14-18`）。这里传 `status=False` 仅校验 `schema_version`，再自做 enum check。`_schema_status` 的 `status` 是 keyword-only 参数，调用形式合法。

### 8.3 `scripts/evidence/final.py`（修改，追加 ~55 LOC）

```python
def seal_publish(
    root: Path,
    identity: PublishIdentity,
    extras: dict[str, object],
) -> dict[str, object]:
    """写入 publish-{target} receipt；manifest 语义与 seal_native_handoff 完全一致。

    extras 字段按 target（status 由 CLI 以 argparse default="approved" 始终显式传入，
    因此 extras 必含 status，可用 require_exact_keys 严格闭合）：
      github-release: release_url, release_id, assets_manifest_sha256, status
      homebrew:        pr_number, pr_branch, pr_url, tap_default_branch, status
    status ∈ {"approved", "no-op"}。
    """
    if (root / "receipt.json").exists() or (root / "SHA256SUMS").exists():
        fail("publish root is already sealed")

    # Strictly close the extras key set per target, consistent with the
    # codebase-wide require_exact_keys discipline: a caller passing a
    # cross-target or misspelled key fails loudly instead of being silently
    # dropped.
    status = require_enum(extras["status"], ALLOWED_STATUS, "publish status")
    if identity.target == "github-release":
        require_exact_keys(
            extras,
            frozenset({"release_url", "release_id", "assets_manifest_sha256", "status"}),
            "github-release publish extras",
        )
        release_url = require_string(extras["release_url"], "release_url")
        release_id = require_uint(extras["release_id"], "release_id", positive=True)
        assets_manifest_sha256 = require_hex(
            extras["assets_manifest_sha256"], 64, "assets_manifest_sha256"
        )
    elif identity.target == "homebrew":
        require_exact_keys(
            extras,
            frozenset({"pr_number", "pr_branch", "pr_url", "tap_default_branch", "status"}),
            "homebrew publish extras",
        )
        pr_number = require_uint(extras["pr_number"], "pr_number")
        pr_branch = require_string(extras["pr_branch"], "pr_branch")
        pr_url = require_string(extras["pr_url"], "pr_url")
        tap_default_branch = require_string(extras["tap_default_branch"], "tap_default_branch")
    else:
        fail(f"unknown publish target: {identity.target}")

    # 与 seal_native_handoff（final.py:144-161）同一模式：manifest 排除
    # {"SHA256SUMS", "receipt.json"}，先写 SHA256SUMS 再写 receipt.json，
    # receipt 里的 manifest_sha256 指向不含自身的树状态。没有第二次
    # write_manifest——write_new 是 O_EXCL（model.py:157-166），重复写
    # SHA256SUMS 会直接 FileExistsError；且含 receipt.json 的 manifest 与
    # receipt 内嵌 digest 构成不可能的不动点。
    top_manifest = write_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))

    receipt: dict[str, object] = {
        "schema_version": 1,
        "status": status,
        "target": identity.target,
        "run_id": identity.run_id,
        "run_attempt": identity.run_attempt,
        "tag": identity.tag,
        "verified_commit": identity.verified_commit,
        "formula_sha256": identity.formula_sha256,
        "manifest_sha256": top_manifest,
        "created_at": utcnow_rfc3339(),
        "seal_actor": "aura-publisher",
    }
    if identity.target == "github-release":
        receipt.update({
            "release_url": release_url,
            "release_id": release_id,
            "assets_manifest_sha256": assets_manifest_sha256,
        })
    else:
        receipt.update({
            "pr_number": pr_number,
            "pr_branch": pr_branch,
            "pr_url": pr_url,
            "tap_default_branch": tap_default_branch,
        })

    validate_publish_receipt(receipt, identity)
    write_new(root / "receipt.json", canonical_json_bytes(receipt))
    return receipt
```

> **关键**：`manifest_sha256` 语义 = "排除 `SHA256SUMS` 与 `receipt.json` 后的树 digest"，验证方用 `verify_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))` 复算比对。这是 `seal_native_handoff` 的精确复刻（`final.py:144` 用同一排除集）。v2 草案里"写完 receipt 再重 stamp 一次 manifest"的设计已删除：它在 `write_new` 的 `O_EXCL` 下必然 raise，且 digest 互相包含无不动点。

### 8.4 `scripts/evidence/receipt.py`

`__all__` 追加 `PublishIdentity` / `validate_publish_receipt`，**与 `HandoffIdentity`/`validate_handoff_receipt` 并列**：

```python
from .receipt_publish import PublishIdentity, validate_publish_receipt

__all__ = (
    "GATE_COMMANDS", "LANE_CONTRACTS", "LANE_JOBS", "RELEASE_PATHS",
    "AggregateIdentity", "FinalIdentity", "GateIdentity",
    "HandoffIdentity", "PublishIdentity",
    "validate_aggregate_receipt", "validate_final_receipt",
    "validate_gate_receipt", "validate_handoff_receipt",
    "validate_lane_receipt", "validate_publish_receipt",
)
```

> `scripts/evidence/__init__.py` 是**空文件**（0 行），re-export 集中在 `receipt.py`；本设计不动 `__init__.py`。

### 8.5 `scripts/evidence/{model,manifest}.py` helpers 复用与新增

新代码**严禁**写新的 hex/tag/sha256 validator，全部用既有：

- 来自 `scripts/evidence/model.py`：
  - `require_hex(value, 40|64, label)` —— 用于 commit 与 sha256（`model.py:106-111`）
  - `require_tag(value)` —— tag 校验（`model.py:114-118`）
  - `require_safe_id(value, label)` —— run_id
  - `require_uint(value, label, positive=...)` —— run_attempt / release_id / pr_number
  - `require_enum(value, choices, label)` —— status
  - `require_string`, `require_exact_keys`, `require_list`
  - `canonical_json_bytes`, `write_new`, `sha256_file`, `decode_json_bytes`
- 来自 `scripts/evidence/manifest.py`（**非** model.py；`final.py:6` 已按此来源 import）：
  - `write_manifest(root, excluded)`（`manifest.py:28-31`）、`verify_manifest(root, excluded)`（`manifest.py:34-42`）

**新增 helper**：`utcnow_rfc3339() -> str`，定义在 `scripts/evidence/model.py`：

```python
from datetime import UTC, datetime

def utcnow_rfc3339() -> str:
    """Return current UTC time as RFC 3339 string with Z suffix."""
    return datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
```

> Python 3.11+ 提供 `datetime.UTC`；GitHub ubuntu-24.04 runner 自带 Python 3.12，符合要求。`from datetime import UTC, datetime` 加在 `model.py` 顶部，与既有 `from __future__ import annotations` 共存。`final.py` 顶部 import 需追加 `PublishIdentity` / `validate_publish_receipt`（来自 `.receipt`）与 `ALLOWED_STATUS`（来自 `.receipt_publish`）、`utcnow_rfc3339`（来自 `.model`）、`require_enum`/`require_string`/`require_uint`/`require_hex`（来自 `.model`，按需并入既有 import 行）。

## 9. Token / 权限矩阵

| 用途 | Token | 来源 | 最小权限 | 注入点 |
|---|---|---|---|---|
| `gh release create/upload/view` (本仓) | `GITHUB_TOKEN` (内置) | Actions 默认 | `contents: write` (job 内) | `permissions: { contents: write }` 在 publish-github-release 块顶 |
| `verify-release-asset.py` 调本仓 release API（含 draft） | `GITHUB_TOKEN` (内置) | Actions 默认 | `contents: read` | env `AURA_RELEASE_TOKEN=${{ secrets.GITHUB_TOKEN }}`（publish-homebrew step 9） |
| `git clone`/`push` (tap) 与 `gh pr list/create/edit` (跨仓) | `HOMEBREW_TAP_TOKEN` | fine-grained PAT（secret 已存在于 `zapsaang/aura`，值不暴露） | repository: only `zapsaang/homebrew-tap`; permissions: `Contents: Read and write`, `Pull requests: Read and write`, `Metadata: Read-only` | `secrets.HOMEBREW_TAP_TOKEN`，并注入到 `GH_TOKEN` env（让 gh 用同一 token） |
| `verify-publish-input.py` 中 `git rev-parse refs/tags/{TAG}^{commit}` | 无 (代码 checkout 已含历史) | — | — | `actions/checkout` `fetch-depth: 0` |

> **为什么 verify-release-asset 不用 `GH_TOKEN`**：publish-homebrew 的 `GH_TOKEN` 是 scoped 到 `zapsaang/homebrew-tap` 的 fine-grained PAT，对本仓 API 无效（401）。两个 token 必须分 env 注入，互不覆盖。

**PAT 生成步骤**：见 §14.1 操作清单。

**Secret hygiene**：

- token 永不出现在 `set-output` / `echo ::set-output` 等 GH Actions 上下文。
- token 不进 PR body（`render-publish-pr-body.py` 严格只接受 sha256 形式资产哈希）。
- token 不进 audit log：`commands:` 步骤用 `env:` 而不是 `run:` 内联。
- askpass 脚本运行时从环境读 token，token 值不落盘（§6.2.1）。

## 10. 幂等 / 重试契约

| 触发情形 | publish-github-release 行为 | publish-homebrew 行为 |
|---|---|---|
| 同 tag push 二刷（重打） | `gh release view` 命中且仍为 draft → `upload --clobber` 全部 9 资产；不存在 → `create --draft`；**已 published → 显式拒绝变更（exit 1）** | `gh pr list --head "aura-${TAG}" --base "$TAP_DEFAULT"` 命中 → `force-with-lease` push 同分支；公式内容一致 → no-op 路径（依然 seal，写 `status="no-op"`） |
| tag 删除后重推 | 残留 release 若为 draft 则被 `gh release view` 命中 → `upload --clobber`；否则新建 draft；已 published 同样拒绝变更 | 同"二刷"行 |
| homebrew-tap 不存在 | — | `git clone` exit 128 → 红；release 已发但公式未推 |
| 同 tag 并发 push | `concurrency: release-${{ github.ref }}, cancel-in-progress: false` 串行；上一轮完成才下一轮开始 | 同左 |
| 公式 PR 被关闭 (PR closed, not merged) | 不受影响 | `--state open` 不命中 → checkout `TAP_DEFAULT` → 新 PR 创建。**注意**：维护者需手工 close 旧 PR；本设计不清理 |
| Release draft 残留（PR 未合并） | asset 已上传但 release 整体为 draft；**draft 只对协作者可见，匿名 URL 404** | `verify-release-asset.py` 走带 token 的 API（draft 可读，§7.2），`publish-homebrew` 正常推 PR；维护者合并 PR 后按 §6.1.1 翻转 |

## 11. 测试矩阵（TDD 贯穿）

### 11.1 新单测（必须先有测试，再写实现）

| 文件 | 用例（最小） |
|---|---|
| `scripts/tests/test_publish_receipt.py` | 保留 receipt schema/no-op/extras/reentry 用例；新增 `seal_publish` 必须复算真实 `assets.txt` digest，传入无关 digest 必须在写 `SHA256SUMS` 前失败 |
| `scripts/tests/test_verify_release_asset.py` | mock API 成功时反序远端列表仍生成名字排序的 authoritative manifest，stdout 为 `{"checks":10,"status":"ok"}`；同为 9 个但错名、任一非 formula 资产 digest 漂移、formula digest 漂移、count/tag/cardinality/HTTP/timeout 均失败且不写输出 manifest |
| `scripts/tests/test_render_publish_pr_body.py` | 保留 4 target/链接/顺序/确定性/输入校验；新增 fork source 的两条 `gh release` 命令都含 `--repo`，configured `OWNER/homebrew-NAME` 渲染成 `brew tap OWNER/NAME`，生产默认保持 `zapsaang/tap` |
| `scripts/tests/test_workflow_contract.py` 扩展 | 1) `test_release_workflow_produces_every_lane_without_assembling` **不变**（publish 不走 lane，9 个 lane 列表保持）；2) 新增 `test_publish_jobs_exist_without_lanes`（草图见下；**不重复 pin 检查**——`test_external_action_refs_are_pinned_to_full_commit_sha`（`test_workflow_contract.py:49-73`）已对 release.yml 全部 `uses:` 行强制 40-hex pin + 非空版本注释，publish job 自动被覆盖） |

> 测试加载模式：用 `importlib.util.spec_from_file_location`（参考 `test_release_machinery.py:22` 的 `_load_module`，30-31 行有两个现有用例）让单测跳过 `from evidence.model import …` 顶层 importing 时机问题。

#### `test_publish_jobs_exist_without_lanes` 草图

```python
def test_publish_jobs_exist_without_lanes(self):
    text = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
    for job in ("publish-github-release", "publish-homebrew"):
        with self.subTest(job=job):
            match = re.search(rf"^  {job}:\n(?P<body>(?:^    .*\n?)+)", text, re.MULTILINE)
            self.assertIsNotNone(match, f"{job} job absent")
            body = match.group("body")
            # 副作用 job 不得召唤 lane；lane 列表由既有 lane 测试锁死在 9 个。
            self.assertNotIn("run-compliance-lane.py", body)
            self.assertNotIn("--job ", body)
    # publish receipt artifact 名必须是内容寻址（与 lane/producer archive 同范式）。
    self.assertIn("aura-publish-github-release-${{ steps.seal-digest.outputs.sha256 }}", text)
    self.assertIn("aura-publish-homebrew-${{ steps.seal-digest.outputs.sha256 }}", text)
```

> **为什么删掉 v3 的 `test_publish_steps_no_unpinned_actions`**：它用自写正则二次实现 pin 检查，与既有 `PINNED_EXTERNAL`（`test_workflow_contract.py:10`）规则重复且可漂移（双源真相 = 技术债）；其块抽取正则 `^\s{2}...\n(?: {4,6}\S.*)` 对 YAML 缩进变体脆弱。增量价值只剩"job 存在 + 不走 lane"，本草图只保留这部分。

### 11.2 既有单测保持不动

- `scripts/tests/test_release_machinery.py` —— 包/校验，无需改
- `scripts/tests/test_evidence_contracts.py` —— receipt_lane/receipt_aggregate，无需改
- `scripts/tests/test_workflow_contract.py:test_release_workflow_produces_every_lane_without_assembling` —— 仍只断言 9 lane

### 11.3 集成测试（dry-run）

> **硬约束**：canonical tag gate（`release.yml:443` + `model.py:12` 的 `TAG` regex）拒绝一切含 `-` 后缀的 tag。**不存在** `v0.0.0-dryrun.1` / `v1.0.0-rc.1` 这类 dry-run tag 能跑通流水线的可能；dry-run 必须在 fork 上用 canonical semver tag 进行（§13 PR-E）。本设计不为此开豁免口子——豁免会削弱正式路径的 gate。

| 触发 | 期望 |
|---|---|
| fork（如 `zapsaang/aura-fork` + fork tap）推 canonical tag `v0.0.0` | `SOURCE_REPOSITORY=${{ github.repository }}` 进入既有 `homebrew-render` tuple；公式 4 个 URL 指向 fork release；API verifier 查询同一 fork；draft 有精确 9 assets；fork tap 上出现 PR；receipt artifacts 两个都在 |
| 同 tag 二刷 | `gh release view`：assets 数不变 = 9（无重复）；upload log 仅含 `--clobber`；tap 侧公式未变 → no-op receipt（`status="no-op"`） |
| 正式 `vX.Y.Z` + 走完 publish-homebrew | tap 上有 PR `aura vX.Y.Z`；release draft；维护者合并 PR + 翻转后，`brew tap zapsaang/tap && brew install aura` 在 macOS+Linux 都跑成功 |

### 11.4 CI 验收（`ci.yml` 中 `compliance-contracts` job 跑 `python3 -B -m unittest discover -s scripts/tests -v`，见 `ci.yml:11-20`）

- `test_publish_receipt.py` / `test_verify_release_asset.py` / `test_render_publish_pr_body.py` 进单测 discover——文件路径必须从 `scripts/tests/` 根直接 import（`from pathlib import Path; sys.path.insert(0, .../scripts)` 模式）。
- `test_workflow_contract.py:test_publish_jobs_exist_without_lanes` 必须绿；既有 `test_external_action_refs_are_pinned_to_full_commit_sha` 自动覆盖 publish job 的 pin 合规。
- `generate-f1-compliance-contract.py --check` **无需新增 output**，因为 registry ID partition 不变；但必须确认 `--check` exit 0（防止静默 schema 漂移）。
- 维持 `ci.yml` 既有 `test-aura-*` cargo-test job 与 lint 步骤不动；新 Python 脚本的覆盖率通过单测用例覆盖（不走额外 gate）。

## 12. 文件改动清单

> 没有行号依赖，所有条目以**文件路径**为单位。任何 git rebase 后只需重跑单测+`test_workflow_contract.py` 验证文本契约不被打破。

| 类别 | 路径 | 动作 |
|---|---|---|
| 新建 | `scripts/evidence/receipt_publish.py` | ~90 LOC；`PublishIdentity` + `validate_publish_receipt` + 两套 target-specific keys + no-op 无 PR 三元组放行 |
| 新建 | `scripts/seal-publish.py` | ~80 LOC；仿 `scripts/seal-native-handoff.py`，调 `evidence.final.seal_publish` |
| 修改 | `scripts/verify-release-asset.py` | GitHub API 精确 9-name/9-digest 核对；从 API 响应写 canonical manifest；`{"checks":10,"status":"ok"}` 契约 |
| 新建 | `scripts/render-publish-pr-body.py` | ~80 LOC；7 参 1 出 |
| 修改 | `scripts/evidence/final.py` | 末尾追加 `seal_publish()` 函数（不动既有 `seal_native_handoff`）；顶部 import 追加 |
| 修改 | `scripts/evidence/model.py` | 追加 `utcnow_rfc3339()` + `from datetime import UTC, datetime` |
| 修改 | `scripts/evidence/receipt.py` | `__all__` 追加 PublishIdentity + validate_publish_receipt |
| 不变 | `scripts/evidence/__init__.py` | 空文件；re-export 集中在 `receipt.py`，不动 |
| 新建 | `scripts/tests/test_publish_receipt.py` | ≥ 13 用例（含 e2e tmpdir + seal_publish 重入 + no-op 三元组 + extras 缺/多 key 严格闭合） |
| 新建 | `scripts/tests/test_verify_release_asset.py` | ≥ 7 用例 |
| 新建 | `scripts/tests/test_render_publish_pr_body.py` | ≥ 4 用例 |
| 修改 | `scripts/tests/test_workflow_contract.py` | **仅**新增 1 方法 `test_publish_jobs_exist_without_lanes`（pin 检查不重复，由既有 `test_external_action_refs_are_pinned_to_full_commit_sha` 覆盖） |
| 不变 | `scripts/evidence/receipt_lane.py` | LANE_JOBS / LANE_CONTRACTS / `_validate_tools` 全部不动 |
| 修改 | `.omo/plans/design-compliance-remediation.md`、`qa/compliance-qa-registry.json`、`qa/plan-sha256.txt` | 仅修改既有 `homebrew-render` command/env，加入 `SOURCE_REPOSITORY`；registry row 总数仍为 114，F1 39-check partition 不变 |
| 不变 | `scripts/evidence/receipt_aggregate.py` | RELEASE_PATHS 不变 |
| 不变 | `scripts/evidence/receipt_handoff.py` | 不动（作为范式存在） |
| 修改 | `.github/workflows/release.yml` | 末尾追加 `publish-github-release` + `publish-homebrew` job；顶层加 `concurrency:`；`homebrew-render`/`homebrew-audit` job 各追加 1 个固定名 tuple artifact 上传 step（§3 表注） |
| 不变 | `.github/workflows/ci.yml` | 跑测试 + lint + cargo tests；不跑 publish |
| 修改 | `docs/homebrew-publish.md` | 本文档 |
| 修改 | `USER_GUIDE.md` | 追"Homebrew upgrade"段 + 维护者操作清单。**注意**：USER_GUIDE 已有 `brew tap zapsaang/tap` 指令（:72-78、:103-108、:287、:497），目前是指向尚不存在仓库的预售文档——PR-F 后才变为真实；本 PR 序列中只在既有段附近补充 upgrade/维护者内容，不重写安装指令 |
| 修改 | `README.md` | PR-F 顺手把 "Homebrew" 段（`README.md:170-174`）从"formula published with each release"改为指向 `brew install zapsaang/tap/aura`，与 USER_GUIDE 对齐，消除文档漂移 |
| 修改 | `docs/adr.md` | 追 **ADR-010** "publish split = lane + non-lane seal" 一段（adr.md 现有 ADR-001~009 三位数编号，下一个是 010；v3 写的"ADR-15"不成立） |
| 新建 | `zapsaang/homebrew-tap` (新仓) | README + Formula/aura.rb 起始版本（详见 §14.1） |

> 实施时 `LANE_JOBS`、`LANE_CONTRACTS`、`_validate_tools`、`execute_lane_rows` 应无 diff；registry 只能有既有 `homebrew-render` 一行的 command/env 变化，不得新增 row。这是隔离性回归测试。

## 13. 落地顺序（每步独立 PR，可回滚）

| PR | 内容 | 验证 | 回滚成本 |
|---|---|---|---|
| **PR-A** | 新增 `scripts/evidence/receipt_publish.py` + 修改 `scripts/evidence/{final,model,receipt}.py` + 新增 `scripts/seal-publish.py` + 新增 `scripts/tests/test_publish_receipt.py` | 单测绿；CI `compliance-contracts` 绿；不触发 release | 仅删除新建 2 文件 + revert 3 个文件 |
| **PR-B** | `scripts/verify-release-asset.py` + `scripts/tests/test_verify_release_asset.py` | 单测绿；CI 绿 | 仅删除新建 2 文件 |
| **PR-C** | `scripts/render-publish-pr-body.py` + `scripts/tests/test_render_publish_pr_body.py` | 单测绿；CI 绿 | 同上 |
| **PR-D** | `release.yml`：`homebrew-render`/`homebrew-audit` 追加 tuple artifact 上传 step；末尾追加 `publish-github-release` + `publish-homebrew` 两 job；顶层加 `concurrency:`；`test_workflow_contract.py` **同 PR** 新增 `test_publish_jobs_exist_without_lanes`（测试与实现同 PR，避免单独先合测试导致 main 红） | CI 单测绿；workflow 语法过 `actionlint` | workflow revert |
| **PR-E** | 在 fork（`zapsaang/aura-fork` + fork tap）推 canonical tag `v0.0.0` dry-run（§11.3）；正式 enabled 前要求 reviewer 双签 | dry-run 全绿；receipt artifacts 可下载检查 | revert commit on tap + delete release |
| **PR-F** | 正式 tag `vX.Y.Z`；维护者人工审 PR 内容 + release draft；合并 tap PR；翻转 release；观察 `gh release view $TAG` | brew install/upgrade 实跑确认 | revert commit on tap + delete release |

> **为什么不把 PR-A/B/C 合并**：每个脚本独立可单测、独立可独立删除，避免一次性大 PR 把 design flaws 一起带上来。
> **为什么测试与 workflow 同 PR（PR-D）**：TDD 的"先红后绿"发生在 PR 内部（commit 顺序：测试 commit 在前）；拆成两个 PR 会让 main 在测试先合时持续红。这是 TDD 与 CI 门禁的正确折中，不是例外。

## 14. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 维护者忘记建 `zapsaang/homebrew-tap` 仓库 | 高 | 高 | §14.1 硬前置；首次跑 PR-D 前必须 init |
| fine-grained PAT 过期 / 遗失 | 中 | 中 | secret 过期前 7 天维护者日历提醒；至少 2 名 maintainer 各持一份；token name 标 `_<seq>` |
| 同 tag 重推期间旧 workflow 还跑 | 低 | 中 | `concurrency: release-${{ github.ref }}, cancel-in-progress: false` 严格串行 |
| Release draft 留挂太久 → brew fetch 404 | 中 | 中 | §6.1.1 的两步分离；维护者合并 PR 即触发 publish；7 天 stale 警报 |
| credential helper 临时文件意外 leak token | 低 | 低 | token 值不落盘（quoted heredoc + 运行时 env 展开）；`chmod 700` 兜底；`$RUNNER_TEMP` job-end 自动清 |
| fork PR 使用同名 head branch 与 tap 内部发布分支碰撞 | 中 | 高 | 查询 `headRepository` + `isCrossRepository`；只有 head repo 精确等于 `TAP_REPOSITORY` 且非 cross-repository 才复用，否则 fail closed |
| `actions/runner-images` 改 runner 默认 PATH 行为 | 低 | 低 | 本设计不依赖 PATH-relative 命令；显式 `gh`/`git` |
| 用户从 mirror 缓存 brew 而拿到 stale formula | 极低 | 中 | sha256 必检；公式与资产 digest 经 `verify-release-asset.py` 双向绑定 |
| 同一 release 二次 `--clobber` 导致同一 name 两次不同 sha256 哈希 | 低 | 中 | step 12 强校验 `assets.length == 9`；`verify-release-asset.py` 在 publish-homebrew 再次核对 digest |
| `GH_TOKEN`（tap PAT）被误用于本仓 API → 401 | 中 | 低 | 本仓 API 走独立 env `AURA_RELEASE_TOKEN`（§9 矩阵） |

> 概率评级改用"维护者主动+被动"分类：high=必须立刻主动；medium=挂日历提醒；low=被动监控告警。`§14.1` 是 high 一类。

### 14.1 维护者前置（hard requirement）——**已全部就绪（实施后状态）**

- [x] **`zapsaang/homebrew-tap`** 仓库存在；branch protection on `main` 已配置：`require pull request` + `require 1 approval`，admin 强制（enforced）。approving user 必须含 release 维护者。
- [x] **`HOMEBREW_TAP_TOKEN`** 已设置：`zapsaang/aura` Settings → Secrets → Actions 存在名为 `HOMEBREW_TAP_TOKEN` 的 secret（fine-grained PAT，scoped 到 `zapsaang/homebrew-tap`，`Contents: Read and write` + `Pull requests: Read and write`）。文档只记录 secret **存在与名称**，不暴露、不暗示其值。
  - token 运维：过期前 7 天维护者日历提醒；至少 2 名 maintainer 各持一份；token name 标 `_<seq>`。
- [x] **`aura-publisher[bot]` identity**：实施使用固定 noreply 邮箱推送，无需单独 bot 账号或 collaborator 配置（见 §6.2 注）。
- [ ] **`aura.rb`** 起始版本手动推送：`curl https://raw.githubusercontent.com/zapsaang/aura/main/deployment/homebrew/aura.rb.in | sed 's/{TAG}/v1.0.0/g' > homebrew-tap/Formula/aura.rb`，然后手动替换全部 4 个 SHA 占位符（`{SHA256_MACOS_ARM}`、`{SHA256_MACOS_X86}`、`{SHA256_LINUX_ARM}`、`{SHA256_LINUX_X86}`），提交到 `main`。后续 PR 即从此基线开始。（v3 的一行 sed 先把 `{SHA256_LINUX_X86}` 替换成字面 `PLACEHOLDER`，与"替换 4 个 `{SHA256_*}` 占位符"的后半句自相矛盾；已修正为只替换 `{TAG}`。）

## 15. 反对意见（提前应对）

- **"为什么不开 GitHub App 而用 PAT？"** — App 安装 + 私钥管理是单独一套基础设施；v1 PAT 够细粒度，secret 已控；GitHub App 可作 v2 升级。
- **"为什么不直推 tap main？"** — tap 是**用户信任入口**（`brew install zapsaang/tap/aura`），错误公式悄推会污染所有用户；PR + 1-review 是 `homebrew-tap` 业界公认门槛。
- **"为什么 publish 不入 registry / lane？"** — 入 lane 必须有 `count_source` (verification)，但 publish 是副作用；强行入 lane 会引入 "constant:0 测试通过但 release 没真发" 的逻辑漏洞。且 `run-evidence-command.py:39-52` 的 worktree 清洁检查在技术上也阻断副作用入 lane。`seal-native-handoff.py` 已示范 non-lane seal 范式。
- **"为什么不直接 `gh release create` 一句话？"** — 一句话不带 audit/tag/asset 上下文；维护者失去 "同 tag 二次重打可解释性"；本设计每步都有 verify gate。
- **"为什么 Release 留 draft，PR 合并后才 publish？"** — 防止坏公式先被 brew 缓存。draft 资产对匿名 404、对协作者可见，正好匹配"维护者可验、用户不可见"的窗口期语义。
- **"为什么 verify-release-asset 用 API 而不是直接探 URL？"** — draft release 的 `releases/download/...` 对匿名请求 404，URL 探活在 draft 阶段必然失败；带 token 的 API digest 核对既支持 draft 又比"200 探活"更强（直接比对内容 digest）。
- **"为什么不开 issue-comment 自动通知 PR？"** — 已经在 PR body 里写 audit log 链接；issue 多余噪音。

## 16. 指针

- `docs/adr.md` —— 现有 ADR-001~009 均为运行时/ABI 决策，**没有** release/evidence 相关 ADR（v3 声称的"现有 §release evidence-bound 决策"段不存在）；新增 **ADR-010** "publish split = lane + non-lane seal" 引用本文件，它是首个发布流水线 ADR
- `docs/ploc.md` —— 不适用（`check-rust-loc.py:163` 只 `rglob("*.rs")`；Python 脚本不受 250 PLOC 限制）
- `docs/tech_blueprint.md` —— 现有 §1~§5（IPC/Top-N/探测/渲染/补救纪要），**无 "CI 流水线" 段**（v3 引用不存在）；如需指针则新增一小段，否则不动
- `USER_GUIDE.md` —— 已有 `brew tap zapsaang/tap` 预售指令（:72-78、:103-108、:287、:497）；PR-F 后变为真实。本设计在其旁补"合并后用户升级步骤"，维护者操作清单仍指向本文件 §14.1
- `scripts/seal-native-handoff.py` —— 本设计的范式
- `scripts/verify-publish-input.py` —— publish input gate（**不动 schema**）
- `docs/homebrew-publish.md` —— 本文件

## 17. 审计记录

**第 2 轮（事实核查）**——逐条比对文档声明与代码/工作流实际内容，已修正：

1. §1 原来写"9 个 lane job（release-*×4、homebrew-render、homebrew-audit、4 个 evidence producer）"——实际 12 个 job、9 个 lane、3 个 producer；`homebrew-render`/`homebrew-audit` 是 job 名不是 lane 名（lane 是 `ubuntu-default`/`macos-default`）；producer 只有 3 个且不跑 `run-compliance-lane.py`。
2. §6.1 原 step 8 的 audit tuple 路径 `lane/homebrew-audit/tuples/homebrew-audit` 不存在；实际在 `lane/macos-default/tuples/homebrew-audit`。
3. tuple 目录只存在于上游 job workspace 并打入**内容寻址名**的 lane archive，publish job 无法按名下载——新增固定名 tuple artifact 上传（§3 表注、§6.1 step 5、§12、PR-D）。
4. `git -c GIT_MASTER=1` 是非法 git config 语法（无 section）；仓内惯例是 `GIT_MASTER=1 git ...` **环境变量**形式（`release.yml:444`、`verify-publish-input.py:63`），已统一。
5. §6.1 原 step 10 的 create 分支漏传 4 个 `.sha256`，与 step 11 的 `assets == 9` 校验自相矛盾（首跑必败）；已改为两分支都传 9 资产。
6. 原 release notes 模板在 create 前调 `gh release view`（首跑鸡生蛋失败）且引用不作为资产的 `SHA256SUMS`（它内嵌于 tarball，`package-release.py:4`）；已改为纯本地 env 渲染 + 内联 digest 表。
7. 原 §10 声称"draft release 非协作者 `gh release view` 仍可见"——错误；draft 仅协作者可见，匿名 asset URL 404。原 §7.2 的未认证 URL 探活在 draft 阶段必然失败，与"留 draft"决策构成死锁；已改为带 token 的 GitHub API digest 核对（§7.2），并相应改 §9 矩阵（新增 `AURA_RELEASE_TOKEN`）与 §10。
8. §6.2 原 step 12 搜索 `aura/${TAG} in:title` 与 step 19 的 PR title `aura ${TAG}`（空格）永不匹配，幂等复用失效；已统一为空格形式。
9. §6.2 原 step 14 的 no-op 判定用 `git diff` 且发生在 `cp` 之前，无法感知新渲染公式的内容变化；已改为 `cmp -s` 直接内容比对。
10. 原设计 no-op 无 PR 时无法提供正数 `pr_number`，与 receipt schema 冲突；已在 validator 显式放行 `no-op + pr_number=0 + 空 pr_url/pr_branch` 三元组（§8.2），其余组合拒绝。
11. §6.2.1 askpass 脚本原样输出 `username=...`/`password=...` 两行——git askpass 协议要求按提示只回凭据值，否则认证必败；已改为 `case "$1"` 分支。
12. §6.1 原表格 step 13-16 重复列出两遍且 receipt 路径不一致（`publish/github-release` vs `publish-github-release`）；已去重并统一为 `publish/{github-release,homebrew}`。
13. §8.3 原 seal_publish 的"写完 receipt 再重 stamp manifest"在 `write_new` 的 `O_EXCL`（`model.py:157-166`）下必然 `FileExistsError`，且 digest 互相包含无不动点；已删除，严格复刻 `seal_native_handoff` 的单次 manifest + 排除集模式。
14. §8.2 代码片段缺 `from dataclasses import dataclass`；已补。
15. §5 引用的 `run-evidence-command.py:SHELL` 不存在（无此常量）；已改为 `CommandSpec.shell`（`run-evidence-command.py:42-49`）。
16. §6.1 原 step 3 的行号引用 `release.yml:228` 有误；artifact 上传名在 `release.yml:53,107,160,213`，公式在 484。
17. 既有 canonical tag gate（`release.yml:441-444`）原文档完全未提，导致 §11.3 的 `v0.0.0-dryrun.1` / `v1.0.0-rc.1` dry-run 计划不可行；已写明约束并把 dry-run 改为 fork + canonical tag（§11.3、PR-E）。
18. §8.4 原文档称修改 `scripts/evidence/__init__.py` "同上 re-export"——该文件是空的（0 行），re-export 集中在 `receipt.py`；已改为不动。
19. §7.1 原 `--formula-sha256 PATH`（读文件）与 `seal-native-handoff.py` 的纯值入参风格不一致；已改为 64hex 值入参。

**第 3 轮（TDD 与技术债）**——校验测试矩阵与设计契约的一致性：

1. §11.1 原用例 2"拒绝 `status == 'approved'`"与设计（approved 是合法值）直接矛盾；已改为正例锁定"approved 必须通过"，并新增 no-op 三元组的正/反用例（现 11 个用例）。
2. §11.1 原 `test_verify_release_asset.py` 用例 3 引用 `--tag` 参数，但 §7.2 入参里没有；新契约已把 `--tag` 列为正式入参并配 drift 用例（现 7 个用例，全部对应 §7.2 真实行为）。
3. 原 PR-D（仅测试）与 PR-E（workflow）分离会让 main 在测试先合时持续红；已合并为 PR-D（测试 commit 在前，TDD 红→绿发生在 PR 内）。
4. 原 §11.3 dry-run 依赖不存在的 tag 豁免；已重写为 fork + canonical tag，并明确"不开豁免口子"。
5. PR body checklist 原顺序（先 `brew install`）与 draft 现实矛盾；已改为 确认资产→合并→翻转→验证（§7.3）。

**第 4 轮（事实复核）**——对 v3 全文再做一次逐条比对，v3 未抓到的新错误：

1. §8.5 把 `write_manifest`/`verify_manifest` 列为 `model.py` helper——二者实际在 `scripts/evidence/manifest.py:28-42`（`model.py` 共 166 行，无此二函数）；`final.py:6` 的既有 import 也证实来源是 `.manifest`。已拆分为 model/manifest 两段归属。
2. §12/§16 的 "ADR-15" 不成立：`docs/adr.md` 编号是三位数 ADR-001~009（无 release/evidence 类 ADR），下一个应为 **ADR-010**。§16 声称的"现有 §release evidence-bound 决策"段在 adr.md 中不存在。两处已修正。
3. §16 引用 `docs/tech_blueprint.md` 的 "CI 流水线" 段——该文件只有 §1~§5（IPC/Top-N/探测/渲染/补救纪要），无 CI 段。已改为"新增小段或不动"。
4. §14.1 起始公式的一行 sed 先把 `{SHA256_LINUX_X86}` 替换成字面 `PLACEHOLDER`，后半句却让维护者"替换全部 4 个 `{SHA256_*}` 占位符"——`{SHA256_LINUX_X86}` 已不存在。已改为 sed 只替换 `{TAG}`（加 `g` 标志）。
5. §12 把 `README.md` 标为"不变"，但 `USER_GUIDE.md:72-78,103-108,287,497` 已写死 `brew tap zapsaang/tap && brew install zapsaang/tap/aura`（目前指向不存在的仓库，属预售文档）；README 的 Homebrew 段（`README.md:170-174`）说 "Install from the rendered formula published with each release"，tap 上线后即陈旧。已把 README 行改为 PR-F 顺手修改，USER_GUIDE 行加注预售现状。
6. §16 对 `check-rust-loc.py` 的声明补上行号证据（`check-rust-loc.py:163` 的 `rglob("*.rs")`）。

**第 5 轮（TDD/技术债复核）**——校验测试矩阵与设计契约的一致性、消除双源真相与规格断链：

1. §11.1 v3 新增的 `test_publish_steps_no_unpinned_actions` 与既有 `test_external_action_refs_are_pinned_to_full_commit_sha`（`test_workflow_contract.py:49-73`，已对 release.yml 全部 `uses:` 行强制 40-hex pin + 非空注释）重复，且自写 YAML 块抽取正则（` {4,6}` 缩进假设）脆弱——双源真相即技术债。已删，改为 `test_publish_jobs_exist_without_lanes`：只断言两 job 存在、不召唤 lane（不含 `run-compliance-lane.py`/`--job `）、receipt artifact 内容寻址名；pin 合规由既有测试自动覆盖。§11.4/§12/§13 PR-D 同步改名。
2. §8.3 `seal_publish` 原实现静默丢弃 extras 里多传的 key（cross-target 混传不报错），违背仓内 `require_exact_keys` strict 风格；且 `status` 用 `extras.get("status", "approved")` 而非显式传参。已改为：CLI 以 argparse `default="approved"` 始终显式传 status，`seal_publish` 顶部对每个 target 做 `require_exact_keys(extras, …)` 严格闭合。§11.1 `test_publish_receipt.py` 相应新增用例 12/13（extras 缺/多 key → fail），用例数 ≥13，§12 同步。
3. §6.1 step 15 的 `$URL`/`$ID`、§6.2 step 21 的 `PR_NUMBER`/`PR_URL`、§6.2 step 19 的 `$RUNNER_TEMP/asset-sha256.txt`、§7.3 的 `--audit-log-url` 在 v3 中均无产生步骤（规格断链，实现者只能猜）。已分别补齐：§6.1 step 14 加 `gh release view --json url,id`；§6.2 step 20 后统一 `gh pr view --json number,url`；§6.2 step 19 前置 shasum 循环；§7.3 注明 `gh api .../jobs` 取 `homebrew-audit` job 的 `html_url` 并降级到 run 级 URL。
4. §6.1 step 3-5 未禁止无 `name` 的 `actions/download-artifact` 全量下载——publish 起跑时仓内已有 12+ 个内容寻址 lane/producer archive，全量下载把它们全拉入 publish job。已在 step 3 写明"逐 `name:` 下载，禁止全量"。
5. §7.2 未固定 urlopen 的 mock 点，`from urllib.request import urlopen` 会让 §11.1 用例 1/2/7 的 `mock.patch("urllib.request.urlopen")` 打空。已补"可测性契约"：必须 `import urllib.request` + 属性调用。

**第 6 轮（实施后复核）**——PR-D/E/F 已落地，对照 `release.yml` 实施逐条校正（见文末"第 6 轮"段）：

1. Secret 名统一为 `HOMEBREW_TAP_TOKEN`；2. v1 无 follow-up workflow（翻转为手工步骤）；3. 已 published release 对 workflow 重跑刻意不可变；4. tap 仓库经 `vars.HOMEBREW_TAP_REPO` 可配置；5. PR 复用按分支 + base 匹配；6. branch protection 与 secret 外部前置已就绪；7. bot identity 定稿。

## 18. 第 6 轮（实施后复核）

1. **Secret 名统一为 `HOMEBREW_TAP_TOKEN`**（原设计名 `HOMEBREW_TAP_PUBLISH_TOKEN`）。该 secret 已存在于 `zapsaang/aura` Settings → Secrets → Actions；文档只记录名称与存在性，不暴露其值。§6.2 step 10、§6.2.1、§9 矩阵、§14.1 全部改为 `HOMEBREW_TAP_TOKEN`。
2. **v1 无 follow-up workflow**：draft → published 的翻转为维护者手工步骤 `gh release edit $TAG --draft=false`。§2 非目标、§4 图、§6.1.1、§7.3 checklist 中原"独立 follow-up workflow"表述已全部删除或改为手工步骤；不声称存在未来的 follow-up workflow。
3. **发布后不可变（实施事实）**：`publish-github-release` 重跑时若 release 已 published（`isDraft != "true"`）即拒绝变更 exit 1；`--clobber` 仅适用于 draft。§6.1.1 与 §10 表已写明。
4. **tap 仓库可配置**：`publish-homebrew` job env `TAP_REPOSITORY: ${{ vars.HOMEBREW_TAP_REPO || 'zapsaang/homebrew-tap' }}`——fork dry-run 可经 repository variable `HOMEBREW_TAP_REPO` 指向 fork tap，正式运行默认 `zapsaang/homebrew-tap`。§6.2 step 13 已写明。
5. **PR 复用按分支匹配**：实施用 `gh pr list --head "aura-${TAG}" --base "$TAP_DEFAULT"` 取代原 `--search "aura ${TAG} in:title"`；§6.2 step 13、§6.2 注、§10 表、§14 风险表已同步。
6. **外部前置已就绪**：`zapsaang/homebrew-tap` main 已配置 branch protection（要求 PR + 1 approval，admin 强制）；`zapsaang/aura` 已配置 Actions secret `HOMEBREW_TAP_TOKEN`。§14.1 勾选项已更新为实施后状态。
7. **bot identity 定稿**：固定 noreply 邮箱 `41898282+aura-publisher[bot]@users.noreply.github.com`；无需单独 bot 账号或 tap collaborator。

## 19. Fork dry-run 与 Oracle 完整性校正

1. **source repository 单一参数**：`render-homebrew-formula.py --source-repository` 默认 `zapsaang/aura`；release workflow 从 `${{ github.repository }}` 注入既有 `ubuntu-default` compliance lane。公式 URL 与 `verify-release-asset.py --repo` 因而在 fork/production 中始终同源。
2. **registry 最小变化**：只更新既有 `homebrew-render` row 的 command/env；114 rows、9 lanes、39-check F1 contract、count source 与 execution context 均未扩展。
3. **PR 防碰撞**：同 branch/base candidate 还必须满足 `headRepository.nameWithOwner == TAP_REPOSITORY` 且 `isCrossRepository == false`；新 PR 创建后直接使用 `gh pr create` 返回 URL 取号，不再进行可能碰撞的第二次 branch lookup。
4. **远端 9 资产闭包**：上传后由 GitHub API 精确校验 9 个名字与全部 digest；receipt 的 `assets.txt` 从 API 响应生成。`seal_publish` 复算该文件 digest，拒绝 unrelated `assets_manifest_sha256`。
5. **draft rerun 元数据同步**：仍为 draft 的既有 release 在 `upload --clobber` 前更新 title 与 notes；已 published release 继续不可变。
6. **人工命令作用域**：PR body 的 `gh release view/edit` 显式含 source `--repo`；Homebrew tap 命令从 configured `TAP_REPOSITORY` 派生，生产输出保持 `zapsaang/tap`。
