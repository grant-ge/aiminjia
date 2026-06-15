#!/usr/bin/env node
import { existsSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const DEFAULT_CASES = [
  {
    id: 'marketing',
    name: '市场营销策划团',
    topic: '618 大促营销节奏怎么排？请先让四位专家各自给一轮观点，再安排一轮交叉点评，最后给我共识、分歧和行动建议。',
    minPeerMessages: 4,
    minLeadValuable: 1,
    requireDeleted: true,
  },
  {
    id: 'strategy',
    name: '战略推演团',
    topic: '是否拓展东南亚市场？请先让各专家独立评估，再让他们互相点评关键分歧，最后给出决策建议。',
    minPeerMessages: 4,
    minLeadValuable: 1,
    requireDeleted: true,
  },
  {
    id: 'debate',
    name: '辩论团',
    topic: '是否应该推出低价版产品？请完成正反双方立论、观察员点评和最终裁决。',
    minPeerMessages: 3,
    minLeadValuable: 1,
    requireDeleted: true,
  },
  {
    id: 'roundtable',
    name: '圆桌讨论团',
    topic: '团队五年后的工作形态会是怎样？请召集 3-5 位合适专家，至少完成一轮观点和一轮互相点评。',
    minPeerMessages: 3,
    minLeadValuable: 0,
    requireDeleted: true,
  },
  {
    id: 'performance-compensation',
    name: '薪酬绩效评审团',
    topic: '调薪方案风险评审：请从薪酬、绩效、HRBP、法务视角先给一轮观点，再交叉点评，最后形成共识和待人工确认事项。',
    minPeerMessages: 4,
    minLeadValuable: 1,
    requireDeleted: true,
  },
  {
    id: 'talent-acquisition',
    name: '招聘评审团',
    topic: '复核三位候选人的匹配度：请先让招聘、用人、面试、人才市场视角各发言，再做交叉点评和最终建议。',
    minPeerMessages: 4,
    minLeadValuable: 1,
    requireDeleted: true,
  },
]

const args = parseArgs(process.argv.slice(2))
const selected = args.caseIds.length
  ? DEFAULT_CASES.filter((item) => args.caseIds.includes(item.id) || args.caseIds.includes(item.name))
  : DEFAULT_CASES

if (selected.length === 0) {
  fail(`No matching cases. Available: ${DEFAULT_CASES.map((item) => item.id).join(', ')}`)
}

const timeoutMs = args.timeoutSec * 1000
const results = []

for (const testCase of selected) {
  const caseStartedAt = Date.now()
  log(`\n=== ${testCase.name} (${testCase.id}) ===`)
  runAijia(['goto', 'expert-teams', '--wait'])
  runAijia(['expert-team-start', '--name', testCase.name])
  const whereAfterStart = runAijiaJson(['where'])
  if (!whereAfterStart.sessionId) {
    fail(`${testCase.name}: no active session after expert-team-start`)
  }
  runAijia(['type-message', testCase.topic])
  runAijia(['send'])
  waitForReply(Math.min(args.waitReplySec, args.timeoutSec))

  const sessionId = runAijiaJson(['where']).sessionId ?? whereAfterStart.sessionId
  const scope = runAijiaJson(['where']).scope
  if (!scope) fail(`${testCase.name}: cannot resolve current user scope from aijia where`)

  const evidence = waitForEvidence({
    scope,
    sessionId,
    testCase,
    deadline: caseStartedAt + timeoutMs,
  })
  results.push({ id: testCase.id, name: testCase.name, sessionId, ...evidence })
  log(
    `ok: peers=${evidence.peerMessages}, leadValuable=${evidence.leadValuableMessages}, deleted=${evidence.deleted}`,
  )
}

console.log(JSON.stringify({ ok: true, results }, null, 2))

function parseArgs(argv) {
  const out = {
    caseIds: [],
    timeoutSec: 480,
    waitReplySec: 180,
  }
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]
    if (arg === '--') {
      continue
    } else if (arg === '--case') {
      out.caseIds.push(argv[++i])
    } else if (arg === '--timeout') {
      out.timeoutSec = Number(argv[++i])
    } else if (arg === '--wait-reply') {
      out.waitReplySec = Number(argv[++i])
    } else if (arg === '--list') {
      console.log(DEFAULT_CASES.map((item) => `${item.id}\t${item.name}`).join('\n'))
      process.exit(0)
    } else {
      fail(`Unknown argument: ${arg}`)
    }
  }
  return out
}

function runAijia(argsForAijia) {
  const result = spawnSync('pnpm', ['exec', 'tauri-pilot', 'aijia', ...argsForAijia], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  if (result.status !== 0) {
    fail(`tauri-pilot aijia ${argsForAijia.join(' ')} failed\n${result.stdout}\n${result.stderr}`)
  }
  return result.stdout.trim()
}

function runAijiaJson(argsForAijia) {
  const text = runAijia([...argsForAijia, '--json'])
  try {
    return JSON.parse(text)
  } catch (err) {
    fail(`Invalid JSON from aijia ${argsForAijia.join(' ')}: ${text}\n${err}`)
  }
}

function waitForReply(timeoutSec) {
  const result = spawnSync(
    'pnpm',
    ['exec', 'tauri-pilot', 'aijia', 'wait-reply', '--timeout', String(timeoutSec), '--json'],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  )
  if (result.status !== 0) {
    fail(`wait-reply failed\n${result.stdout}\n${result.stderr}`)
  }
  const parsed = JSON.parse(result.stdout)
  if (!parsed.ok) fail(`wait-reply timed out: ${result.stdout}`)
}

function waitForEvidence({ scope, sessionId, testCase, deadline }) {
  let last = null
  while (Date.now() < deadline) {
    last = inspectEvidence(scope, sessionId)
    if (
      last.peerMessages >= testCase.minPeerMessages &&
      last.leadValuableMessages >= testCase.minLeadValuable &&
      (!testCase.requireDeleted || last.deleted)
    ) {
      return last
    }
    sleep(2000)
  }
  fail(
    `${testCase.name}: evidence did not satisfy requirements before timeout. last=${JSON.stringify(
      last,
      null,
      2,
    )}`,
  )
}

function inspectEvidence(scope, sessionId) {
  const convDir = join(homedir(), '.renlijia', 'users', scope, 'conversations', sessionId)
  const teamsDir = join(convDir, 'teams')
  const teamNames = listTeamDirs(teamsDir)
  const totals = {
    teams: teamNames,
    peerMessages: 0,
    leadMessages: 0,
    leadValuableMessages: 0,
    leadLowSignalMessages: 0,
    deleted: false,
  }
  for (const teamName of teamNames) {
    const teamDir = join(teamsDir, teamName)
    const configPath = join(teamDir, 'config.json')
    const chatPath = join(teamDir, 'team-chat.jsonl')
    if (existsSync(configPath)) {
      const config = JSON.parse(readFileSync(configPath, 'utf8'))
      totals.deleted ||= Boolean(config.deleted_at)
    }
    if (existsSync(chatPath)) {
      for (const row of readJsonl(chatPath)) {
        if (row.from === 'team-lead') {
          totals.leadMessages += 1
          if (isLowSignalLead(row.text ?? '')) totals.leadLowSignalMessages += 1
          else totals.leadValuableMessages += 1
        } else if (row.to === 'team-lead') {
          totals.peerMessages += 1
        }
      }
    }
  }
  return totals
}

function listTeamDirs(teamsDir) {
  if (!existsSync(teamsDir)) return []
  return spawnSync('find', [teamsDir, '-mindepth', '1', '-maxdepth', '1', '-type', 'd'], {
    encoding: 'utf8',
  })
    .stdout.split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((path) => path.split('/').at(-1))
}

function readJsonl(path) {
  return readFileSync(path, 'utf8')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => line.split('\t')[0])
    .map((line) => JSON.parse(line))
}

function isLowSignalLead(text) {
  return /收到|已记录|正在等待|保持等待|尚未提交|尚未发言|其他成员已就位|请尽快|准备好了吗/.test(
    text.replace(/\s+/g, ' ').trim(),
  )
}

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms)
}

function log(message) {
  process.stderr.write(`${message}\n`)
}

function fail(message) {
  console.error(message)
  process.exit(1)
}
