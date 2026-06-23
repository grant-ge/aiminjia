#!/usr/bin/env node
import { copyFileSync, existsSync, linkSync, unlinkSync } from 'node:fs'
import { basename, dirname, join } from 'node:path'
import { spawn } from 'node:child_process'

const CARGO_PACKAGE_NAME = 'aijia'
const appName = process.env.AIJIA_DEV_APP_NAME

if (!appName) {
  console.error('AIJIA_DEV_APP_NAME is required')
  process.exit(1)
}

const cargoRunArgs = process.argv.slice(2)
if (cargoRunArgs[0] !== 'run') {
  console.error(`Expected cargo run args, got: ${JSON.stringify(cargoRunArgs)}`)
  process.exit(1)
}

const separatorIndex = cargoRunArgs.indexOf('--')
const cargoOptions =
  separatorIndex === -1 ? cargoRunArgs.slice(1) : cargoRunArgs.slice(1, separatorIndex)
const appArgs = separatorIndex === -1 ? [] : cargoRunArgs.slice(separatorIndex + 1)
const cargoBuildArgs = ['build', ...cargoOptions]

const buildExitCode = await run('cargo', cargoBuildArgs)
if (buildExitCode !== 0) {
  process.exit(buildExitCode)
}

const sourceExecutable = resolveCargoExecutable(cargoOptions)
const executableDir = dirname(sourceExecutable)
const devExecutable = join(executableDir, sanitizeExecutableName(appName))

if (sourceExecutable !== devExecutable) {
  replaceDevExecutable(sourceExecutable, devExecutable)
}

if (process.env.AIJIA_TAURI_RUNNER_DEBUG === '1') {
  console.error(
    JSON.stringify(
      {
        cargoBuildArgs,
        sourceExecutable,
        devExecutable,
        appArgs,
      },
      null,
      2,
    ),
  )
}

process.exit(await run(devExecutable, appArgs, `Failed to run ${basename(devExecutable)}`))

function resolveCargoExecutable(cargoOptions) {
  const targetDir = process.env.CARGO_TARGET_DIR || 'target'
  const targetTriple = optionValue(cargoOptions, '--target')
  const profile = cargoOptions.includes('--release') ? 'release' : 'debug'
  return join(process.cwd(), targetDir, targetTriple ?? '', profile, CARGO_PACKAGE_NAME)
}

function optionValue(args, name) {
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index]
    if (arg === name) return args[index + 1]
    if (arg.startsWith(`${name}=`)) return arg.slice(name.length + 1)
  }

  return null
}

function sanitizeExecutableName(name) {
  return name.replace(/[^A-Za-z0-9._-]/g, '-')
}

function replaceDevExecutable(source, target) {
  if (existsSync(target)) {
    unlinkSync(target)
  }

  try {
    linkSync(source, target)
    return
  } catch {
    copyFileSync(source, target)
  }
}

function run(command, args, errorPrefix = `Failed to run ${command}`) {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      cwd: process.cwd(),
      env: process.env,
      stdio: 'inherit',
    })

    child.on('error', (error) => {
      console.error(`${errorPrefix}: ${error.message}`)
      resolve(1)
    })

    child.on('exit', (code, signal) => {
      if (signal) {
        process.kill(process.pid, signal)
        return
      }
      resolve(code ?? 1)
    })
  })
}
