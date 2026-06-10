#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const root = process.cwd();
const agentsCasesPath = path.join(root, '.agents/skills/userwiki/references/qa-smoke-cases.json');
const claudeCasesPath = path.join(root, '.claude/skills/userwiki/references/qa-smoke-cases.json');
const args = process.argv.slice(2);

function usage() {
  console.log(`Usage:
  node scripts/run-userwiki-qa-smoke.mjs --validate-only
  node scripts/run-userwiki-qa-smoke.mjs --list
  node scripts/run-userwiki-qa-smoke.mjs --case <id>
  node scripts/run-userwiki-qa-smoke.mjs --case <id> --answer <path>
  node scripts/run-userwiki-qa-smoke.mjs --case <id> --answer -
  node scripts/run-userwiki-qa-smoke.mjs --case <id> --prompt-out <path>
  node scripts/run-userwiki-qa-smoke.mjs --default [--timeout-ms 180000]
  node scripts/run-userwiki-qa-smoke.mjs --all

Default without flags validates the QA fixture schema only.

Environment:
  USERWIKI_QA_TIMEOUT_MS      Override codex exec timeout.
  USERWIKI_QA_CODEX_COMMAND   Override the Codex CLI executable used for live QA.`);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function validateCases(cases) {
  const errors = [];
  const seen = new Set();

  if (!Array.isArray(cases) || cases.length === 0) {
    errors.push('qa-smoke-cases.json must be a non-empty array');
    return errors;
  }

  for (const testCase of cases) {
    if (!testCase || typeof testCase !== 'object') {
      errors.push('case must be an object');
      continue;
    }
    if (!testCase.id || !/^[a-z0-9-]+$/.test(testCase.id)) {
      errors.push(`case has invalid id: ${JSON.stringify(testCase.id)}`);
    }
    if (seen.has(testCase.id)) errors.push(`duplicate case id: ${testCase.id}`);
    seen.add(testCase.id);
    if (!testCase.question || typeof testCase.question !== 'string') {
      errors.push(`${testCase.id} missing question`);
    }
    if (!Array.isArray(testCase.requiredTerms) || testCase.requiredTerms.length === 0) {
      errors.push(`${testCase.id} missing requiredTerms`);
    }
    if (testCase.requiredAnyTerms && (
      !Array.isArray(testCase.requiredAnyTerms)
      || testCase.requiredAnyTerms.some((group) => !Array.isArray(group) || group.length === 0)
    )) {
      errors.push(`${testCase.id} requiredAnyTerms must be an array of non-empty arrays`);
    }
    if (testCase.forbiddenTerms && !Array.isArray(testCase.forbiddenTerms)) {
      errors.push(`${testCase.id} forbiddenTerms must be an array`);
    }
  }

  return errors;
}

function selectCases(cases) {
  const caseIndex = args.indexOf('--case');
  if (caseIndex !== -1) {
    const id = args[caseIndex + 1];
    const found = cases.find((testCase) => testCase.id === id);
    if (!found) throw new Error(`unknown case id: ${id}`);
    return [found];
  }

  if (args.includes('--all')) return cases;
  return cases.filter((testCase) => testCase.default);
}

function timeoutMs() {
  const index = args.indexOf('--timeout-ms');
  const raw = index === -1 ? process.env.USERWIKI_QA_TIMEOUT_MS : args[index + 1];
  const value = Number(raw || 180000);
  return Number.isFinite(value) && value > 0 ? value : 180000;
}

function buildPrompt(testCase) {
  return [
    testCase.question,
    '',
    '只读验证，不要修改文件，不要打开浏览器。',
    '请按 userwiki 规则回答，并尽量给出可点击的本地文件路径。'
  ].join('\n');
}

function codexCommand() {
  return process.env.USERWIKI_QA_CODEX_COMMAND || 'codex';
}

function codexExecutionHint(command) {
  return [
    `提示：当前环境无法直接执行 ${command} 时，先用 --prompt-out <path> 导出同一题 prompt，`,
    '再用 --answer <path> 或 --answer - 评分；如果有可执行的 Codex CLI，',
    '请通过 USERWIKI_QA_CODEX_COMMAND 指向它。'
  ].join('');
}

function isAccessDeniedText(text) {
  return /Access is denied|EPERM|EACCES/i.test(text);
}

function evaluateAnswer(testCase, answer, outputPath) {
  const missing = testCase.requiredTerms.filter((term) => !answer.includes(term));
  const missingAny = (testCase.requiredAnyTerms || [])
    .filter((group) => !group.some((term) => answer.includes(term)))
    .map((group) => `missing one of: ${group.join(' | ')}`);
  const forbidden = (testCase.forbiddenTerms || []).filter((term) => answer.includes(term));

  return {
    id: testCase.id,
    ok: missing.length === 0 && missingAny.length === 0 && forbidden.length === 0,
    outputPath,
    errors: [
      ...missing.map((term) => `missing required term: ${term}`),
      ...missingAny,
      ...forbidden.map((term) => `contains forbidden term: ${term}`)
    ]
  };
}

function runCase(testCase) {
  const outputPath = path.join(os.tmpdir(), `userwiki-qa-${testCase.id}-${Date.now()}.md`);
  const limit = timeoutMs();
  const command = codexCommand();
  const prompt = buildPrompt(testCase);

  const result = spawnSync(command, [
    'exec',
    '--cd',
    root,
    '--sandbox',
    'read-only',
    '--ephemeral',
    '--output-last-message',
    outputPath,
    prompt
  ], {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 12,
    timeout: limit
  });

  if (result.error?.code === 'ETIMEDOUT') {
    return {
      id: testCase.id,
      ok: false,
      outputPath,
      errors: [`${command} exec timed out after ${limit}ms`]
    };
  }

  if (result.error) {
    const errors = [
      `${command} exec failed: ${result.error.code || result.error.name || 'unknown error'}`,
      result.error.message
    ].filter(Boolean);
    if (result.error.code === 'EPERM' || result.error.code === 'EACCES') {
      errors.push(codexExecutionHint(command));
    }

    return {
      id: testCase.id,
      ok: false,
      outputPath,
      errors
    };
  }

  if (result.status !== 0) {
    const stderr = String(result.stderr || '').trim();
    const stdout = String(result.stdout || '').trim();
    const errors = [
      `${command} exec exited with status ${result.status}`,
      stderr,
      stdout
    ].filter(Boolean);
    if (isAccessDeniedText(`${stderr}\n${stdout}`)) {
      errors.push(codexExecutionHint(command));
    }

    return {
      id: testCase.id,
      ok: false,
      outputPath,
      errors
    };
  }

  const answer = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, 'utf8') : '';
  return evaluateAnswer(testCase, answer, outputPath);
}

if (!fs.existsSync(agentsCasesPath)) {
  console.error(`missing ${path.relative(root, agentsCasesPath)}`);
  process.exit(1);
}

const cases = readJson(agentsCasesPath);
const errors = validateCases(cases);

if (fs.existsSync(claudeCasesPath)) {
  const agentsText = fs.readFileSync(agentsCasesPath, 'utf8');
  const claudeText = fs.readFileSync(claudeCasesPath, 'utf8');
  if (agentsText !== claudeText) errors.push('qa-smoke-cases.json mirror mismatch between .agents and .claude');
}

if (errors.length) {
  console.error(`UserWiki QA fixture validation failed (${errors.length} issue${errors.length === 1 ? '' : 's'}):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

if (args.includes('--help')) {
  usage();
  process.exit(0);
}

if (args.includes('--list')) {
  for (const testCase of cases) {
    console.log(`${testCase.id}${testCase.default ? ' [default]' : ''}: ${testCase.question}`);
  }
  process.exit(0);
}

if (args.length === 0 || args.includes('--validate-only')) {
  console.log(`UserWiki QA fixtures are valid. Cases: ${cases.length}`);
  process.exit(0);
}

const selected = selectCases(cases);
const answerIndex = args.indexOf('--answer');
const promptOutIndex = args.indexOf('--prompt-out');
let results;

if (answerIndex !== -1 && promptOutIndex !== -1) {
  throw new Error('--answer and --prompt-out cannot be used together');
}

if (promptOutIndex !== -1) {
  const promptPath = args[promptOutIndex + 1];
  if (!promptPath) throw new Error('--prompt-out requires a path');
  if (selected.length !== 1) throw new Error('--prompt-out requires exactly one selected case');
  const resolvedPromptPath = path.resolve(root, promptPath);
  fs.mkdirSync(path.dirname(resolvedPromptPath), { recursive: true });
  fs.writeFileSync(resolvedPromptPath, buildPrompt(selected[0]), 'utf8');
  console.log(`WROTE ${selected[0].id} prompt=${resolvedPromptPath}`);
  process.exit(0);
}

if (answerIndex !== -1) {
  const answerPath = args[answerIndex + 1];
  if (!answerPath) throw new Error('--answer requires a path');
  if (selected.length !== 1) throw new Error('--answer requires exactly one selected case');
  const answer = answerPath === '-'
    ? fs.readFileSync(0, 'utf8')
    : fs.readFileSync(path.resolve(root, answerPath), 'utf8');
  results = [evaluateAnswer(selected[0], answer, answerPath)];
} else {
  results = [];
  for (const testCase of selected) {
    console.log(`RUN ${testCase.id}`);
    const result = runCase(testCase);
    results.push(result);
    console.log(`${result.ok ? 'PASS' : 'FAIL'} ${result.id} output=${result.outputPath}`);
    for (const error of result.errors) console.log(`  - ${error}`);
  }
}

const failed = results.filter((result) => !result.ok);

if (answerIndex !== -1) {
  for (const result of results) {
    console.log(`${result.ok ? 'PASS' : 'FAIL'} ${result.id} output=${result.outputPath}`);
    for (const error of result.errors) console.log(`  - ${error}`);
  }
}

if (failed.length) process.exit(1);
