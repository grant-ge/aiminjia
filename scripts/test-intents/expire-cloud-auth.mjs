#!/usr/bin/env node
import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const EXPIRY = '2000-01-01T00:00:00Z'
const FIELDS = ['accessExpiresAt', 'refreshExpiresAt', 'sessionKeyExpiresAt']

function usage() {
  console.log(`Usage:
  node scripts/test-intents/expire-cloud-auth.mjs status [--home <path>]
  node scripts/test-intents/expire-cloud-auth.mjs expire [--home <path>] [--at <iso>] [--backup-path <path>] [--overwrite-backup]
  node scripts/test-intents/expire-cloud-auth.mjs restore [--home <path>] [--backup-path <path>] [--delete-backup]

Expires all persisted cloud auth timestamps for AIjia intent tests.
The script prints metadata only; it never prints tokens or session keys.`)
}

function parseArgs(argv) {
  const args = { command: argv[2], home: null, at: EXPIRY, backupPath: null, overwriteBackup: false, deleteBackup: false }
  if (args.command === '--help' || args.command === '-h') {
    args.command = 'help'
  }
  for (let i = 3; i < argv.length; i += 1) {
    const arg = argv[i]
    if (arg === '--home') {
      args.home = argv[++i]
    } else if (arg === '--at') {
      args.at = argv[++i]
    } else if (arg === '--backup-path') {
      args.backupPath = argv[++i]
    } else if (arg === '--overwrite-backup') {
      args.overwriteBackup = true
    } else if (arg === '--delete-backup') {
      args.deleteBackup = true
    } else if (arg === '--help' || arg === '-h') {
      args.command = 'help'
    } else {
      throw new Error(`Unknown argument: ${arg}`)
    }
  }
  return args
}

function resolveHome(input) {
  if (input) return path.resolve(input.replace(/^~(?=$|\/)/, os.homedir()))
  if (process.env.AIJIA_HOME) return path.resolve(process.env.AIJIA_HOME)
  return path.join(os.homedir(), '.renlijia')
}

function pathsFor(home, backupPath) {
  const authPath = path.join(home, 'global', 'auth', 'cloud_auth')
  return {
    home,
    authPath,
    keyPath: path.join(home, 'crypto', 'master.key'),
    backupPath: backupPath ? path.resolve(backupPath.replace(/^~(?=$|\/)/, os.homedir())) : `${authPath}.intent-expire.bak`,
  }
}

function isEncryptedBlob(raw) {
  return /^[0-9a-fA-F]{24}:[0-9a-fA-F]+$/.test(raw.trim())
}

function readMasterKey(keyPath) {
  const keyHex = fs.readFileSync(keyPath, 'utf8').trim()
  const key = Buffer.from(keyHex, 'hex')
  if (key.length !== 32) {
    throw new Error(`Invalid master key length at ${keyPath}; expected 32 bytes`)
  }
  return key
}

function decryptBlob(raw, keyPath) {
  const [nonceHex, cipherHex] = raw.trim().split(':')
  const nonce = Buffer.from(nonceHex, 'hex')
  const encrypted = Buffer.from(cipherHex, 'hex')
  if (nonce.length !== 12) {
    throw new Error(`Invalid nonce length; expected 12 bytes, got ${nonce.length}`)
  }
  if (encrypted.length < 17) {
    throw new Error('Invalid ciphertext length; expected ciphertext plus 16-byte auth tag')
  }

  const tag = encrypted.subarray(encrypted.length - 16)
  const ciphertext = encrypted.subarray(0, encrypted.length - 16)
  const decipher = crypto.createDecipheriv('aes-256-gcm', readMasterKey(keyPath), nonce)
  decipher.setAuthTag(tag)
  return Buffer.concat([decipher.update(ciphertext), decipher.final()]).toString('utf8')
}

function encryptBlob(json, keyPath) {
  const nonce = crypto.randomBytes(12)
  const cipher = crypto.createCipheriv('aes-256-gcm', readMasterKey(keyPath), nonce)
  const ciphertext = Buffer.concat([cipher.update(json, 'utf8'), cipher.final()])
  const tag = cipher.getAuthTag()
  return `${nonce.toString('hex')}:${Buffer.concat([ciphertext, tag]).toString('hex')}`
}

function readAuth({ authPath, keyPath }) {
  if (!fs.existsSync(authPath)) {
    throw new Error(`cloud_auth not found: ${authPath}`)
  }
  const raw = fs.readFileSync(authPath, 'utf8')
  const encrypted = isEncryptedBlob(raw)
  const json = encrypted ? decryptBlob(raw, keyPath) : raw
  return { raw, encrypted, auth: JSON.parse(json) }
}

function writeFileAtomic(filePath, value) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  const tmp = `${filePath}.tmp-${process.pid}`
  fs.writeFileSync(tmp, value)
  fs.renameSync(tmp, filePath)
}

function serializeAuth(auth, encrypted, keyPath) {
  const json = JSON.stringify(auth)
  return encrypted ? encryptBlob(json, keyPath) : json
}

function statusPayload(paths, info) {
  const now = Date.now()
  const expires = Object.fromEntries(FIELDS.map((field) => [field, info.auth[field] ?? null]))
  return {
    ok: true,
    home: paths.home,
    cloudAuthPath: paths.authPath,
    encrypted: info.encrypted,
    ...expires,
    expiredAll: FIELDS.every((field) => {
      const value = info.auth[field]
      return typeof value === 'string' && Date.parse(value) <= now
    }),
  }
}

function printJson(payload) {
  console.log(JSON.stringify(payload, null, 2))
}

function expire(paths, at, overwriteBackup) {
  const info = readAuth(paths)
  if (!Number.isFinite(Date.parse(at))) {
    throw new Error(`Invalid --at datetime: ${at}`)
  }

  if (fs.existsSync(paths.backupPath) && !overwriteBackup) {
    throw new Error(`Backup already exists: ${paths.backupPath}; pass --overwrite-backup to replace it`)
  }
  fs.mkdirSync(path.dirname(paths.backupPath), { recursive: true })
  fs.copyFileSync(paths.authPath, paths.backupPath)

  for (const field of FIELDS) {
    if (!(field in info.auth)) {
      throw new Error(`cloud_auth is missing field ${field}`)
    }
    info.auth[field] = at
  }
  writeFileAtomic(paths.authPath, serializeAuth(info.auth, info.encrypted, paths.keyPath))
  const updated = readAuth(paths)
  printJson({ action: 'expire', backupPath: paths.backupPath, ...statusPayload(paths, updated) })
}

function restore(paths, deleteBackup) {
  if (!fs.existsSync(paths.backupPath)) {
    throw new Error(`Backup not found: ${paths.backupPath}`)
  }
  fs.copyFileSync(paths.backupPath, paths.authPath)
  if (deleteBackup) {
    fs.unlinkSync(paths.backupPath)
  }
  const info = readAuth(paths)
  printJson({ action: 'restore', backupPath: paths.backupPath, backupDeleted: deleteBackup, ...statusPayload(paths, info) })
}

function main() {
  const args = parseArgs(process.argv)
  if (!args.command || args.command === 'help') {
    usage()
    return
  }
  const paths = pathsFor(resolveHome(args.home), args.backupPath)

  if (args.command === 'status') {
    printJson({ action: 'status', ...statusPayload(paths, readAuth(paths)) })
  } else if (args.command === 'expire') {
    expire(paths, args.at, args.overwriteBackup)
  } else if (args.command === 'restore') {
    restore(paths, args.deleteBackup)
  } else {
    throw new Error(`Unknown command: ${args.command}`)
  }
}

try {
  main()
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exit(1)
}
