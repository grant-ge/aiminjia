@echo off
if defined AIJIA_DEV_NODE (
  "%AIJIA_DEV_NODE%" "%~dp0tauri-dev-runner.mjs" %*
) else (
  node "%~dp0tauri-dev-runner.mjs" %*
)
