# Global skills artifact 发布脚本

`scripts/skills/build-skills-artifact.sh` 负责把 app 自带的全局 skills 目录打包成独立 zip，并生成可合并到发布 manifest 的 fragment。它只处理 skills artifact，不混入 runtime、Python、Playwright 或其他安装流程。

## 用法

```bash
scripts/skills/build-skills-artifact.sh <skills-root> <bundle-version> <output-dir>
```

参数说明：

- `<skills-root>`：skills 根目录；其一级子目录代表单个 skill。
- `<bundle-version>`：本次全局 skills bundle 版本，会写入 manifest fragment；必须匹配 `^[A-Za-z0-9._-]+$`，不能以 `.` 开头，且不能包含 `/` 或 `\`，避免路径穿越和 manifest 注入。
- `<output-dir>`：输出目录，脚本会自动创建。

合法 skill 目录要求：

- 目录名必须匹配 `^[a-z0-9][a-z0-9_-]{0,63}$`。
- 目录内必须包含 `SKILL.md`。
- 目录内不允许包含 symlink；发现任何符号链接都会失败，避免发布 artifact 打包外部文件内容。
- 以 `.` 或 `_` 开头的目录会被跳过，便于放置本地草稿或私有素材。
- 如果没有任何合法 skill，脚本会失败。

## 输出

脚本会先校验 `bundle-version`，再用 `python3` 的 `json` 模块生成 manifest fragment；如果系统缺少 `python3` 会明确失败，不会用未转义字符串拼接 JSON。

脚本会在 `<output-dir>` 生成：

- `renlijia-global-skills-<version>.zip`：zip 根目录直接包含各 skill 目录；打包使用 `zip -X -qr`，去除额外文件属性，并排除 `.DS_Store` 与 `__MACOSX` 元数据目录。
- `renlijia-global-skills-<version>.zip.manifest-fragment.json`：包含 `bundleVersion`、`artifact.url`、`artifact.sha256`、`artifact.sizeBytes` 和 `artifact.archiveFormat="zip"`。

artifact URL 默认使用：

```text
https://rlj-cdn.oss-cn-hangzhou.aliyuncs.com/lotus/skills
```

可通过环境变量覆盖：

```bash
RENLIJIA_GLOBAL_SKILLS_BASE_URL=https://example.com/lotus/skills \
  scripts/skills/build-skills-artifact.sh <skills-root> <bundle-version> <output-dir>
```

为了兼容旧命名，也支持 `RENLJ_GLOBAL_SKILLS_BASE_URL`。如果两个变量同时存在，优先使用 `RENLIJIA_GLOBAL_SKILLS_BASE_URL`。

## 发布流程

1. 准备 app 自带全局 skills 根目录，确保每个要发布的 skill 都有 `SKILL.md`。
2. 运行脚本生成 zip 和 manifest fragment。
3. 上传 `renlijia-global-skills-<version>.zip` 到 CDN 的 skills 路径。
4. 将 `.manifest-fragment.json` 合并到生产 manifest 或发布索引中。
5. 发布 manifest，让客户端按新版本发现并同步全局 skills。

## App 侧同步策略

App 侧后台同步应读取 manifest、下载 zip、校验 sha256 和 size，再解压到 staging 目录并原子切换。该同步流程不应阻塞主启动路径：网络失败、校验失败或解压失败时保留当前可用 skills，并在后台重试或等待下次检查。

## 本地 smoke test 示例

```bash
tmpdir="$(mktemp -d)"
mkdir -p "$tmpdir/skills/demo-skill" "$tmpdir/out"
printf '# Demo skill\n' > "$tmpdir/skills/demo-skill/SKILL.md"
scripts/skills/build-skills-artifact.sh "$tmpdir/skills" 0.0.0-smoke "$tmpdir/out"
test -f "$tmpdir/out/renlijia-global-skills-0.0.0-smoke.zip"
manifest="$tmpdir/out/renlijia-global-skills-0.0.0-smoke.zip.manifest-fragment.json"
test -f "$manifest"
python3 -m json.tool "$manifest" >/dev/null
```
