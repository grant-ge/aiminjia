# Runtime artifact 发布脚本

`build-runtime-artifact.sh` 只负责把已经准备好的 runtime 目录打包成 `tar.gz` 并生成 manifest fragment。它不会在用户机器上安装 Node/Python，也不会调用 npm/pip/uv 去拼装生产 runtime。

生产流程：

1. CI 或发布机准备完整目录：`node/`、`python/`、`uv/`。
2. 运行：`scripts/runtime/build-runtime-artifact.sh <runtime-dir> <bundle-version> <output-dir>`。
3. 上传生成的 `renlijia-primary-runtime-<version>-<platform>.tar.gz` 到下载源。
4. 把 `.manifest-fragment.json` 合并进生产 `runtime-manifest.json`。

应用侧只下载 manifest 指向的 artifact，校验 sha256，解压 staging，smoke test，再切 `current`。
