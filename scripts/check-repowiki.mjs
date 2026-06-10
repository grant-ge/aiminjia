#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const requiredFiles = [
  'docs/repo-wiki/README.md',
  'docs/repo-wiki/index.md',
  'docs/repo-wiki/sources.md',
  'docs/repo-wiki/architecture-map.md',
  'docs/repo-wiki/runtime-map.md',
  'docs/repo-wiki/frontend-map.md',
  'docs/repo-wiki/testing-and-commands.md',
  'docs/repo-wiki/decision-index.md',
  'docs/repo-wiki/coverage-manifest.md',
  'docs/repo-wiki/writeback-queue.md',
  'docs/repo-wiki/log.md',
  '.agents/skills/userwiki/SKILL.md',
  '.agents/skills/userwiki/references/install.md',
  '.agents/skills/userwiki/references/usage.md',
  '.agents/skills/userwiki/references/llm-wiki-principles.md',
  '.agents/skills/userwiki/references/qa-playbook.md',
  '.agents/skills/userwiki/references/qa-examples.md',
  '.agents/skills/userwiki/references/qa-smoke-cases.json',
  '.agents/skills/userwiki/references/maintenance-routing.md',
  '.agents/skills/userwiki/references/troubleshooting.md',
  '.agents/skills/wiki-maintainer/SKILL.md',
  '.agents/skills/wiki-maintainer/references/source-policy.md',
  '.agents/skills/wiki-maintainer/references/enhancement-schema.md',
  '.agents/skills/wiki-maintainer/references/validation-rubric.md',
  '.agents/skills/wiki-maintainer/references/subagent-workflow.md',
  '.claude/skills/userwiki/SKILL.md',
  '.claude/skills/userwiki/references/install.md',
  '.claude/skills/userwiki/references/usage.md',
  '.claude/skills/userwiki/references/llm-wiki-principles.md',
  '.claude/skills/userwiki/references/qa-playbook.md',
  '.claude/skills/userwiki/references/qa-examples.md',
  '.claude/skills/userwiki/references/qa-smoke-cases.json',
  '.claude/skills/userwiki/references/maintenance-routing.md',
  '.claude/skills/userwiki/references/troubleshooting.md',
  '.claude/skills/wiki-maintainer/SKILL.md',
  '.claude/skills/wiki-maintainer/references/source-policy.md',
  '.claude/skills/wiki-maintainer/references/enhancement-schema.md',
  '.claude/skills/wiki-maintainer/references/validation-rubric.md',
  '.claude/skills/wiki-maintainer/references/subagent-workflow.md',
  '.understand-anything/config.json',
  '.understand-anything/knowledge-graph.json',
  'scripts/apply-understand-enhancements.mjs',
  'scripts/run-userwiki-qa-smoke.mjs',
];

const errors = [];

function exists(relPath) {
  return fs.existsSync(path.join(root, relPath));
}

function splitMarkdownRow(line) {
  const trimmed = line.trim();
  if (!trimmed.startsWith('|') || !trimmed.endsWith('|')) return null;
  return trimmed.slice(1, -1).split('|').map((cell) => cell.trim());
}

function isMarkdownSeparator(cells) {
  return cells.length > 0 && cells.every((cell) => /^:?-{3,}:?$/.test(cell.replace(/\s/g, '')));
}

function parseMarkdownTables(text) {
  const tables = [];
  const lines = text.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const headers = splitMarkdownRow(lines[index]);
    const separator = splitMarkdownRow(lines[index + 1] || '');
    if (!headers || !separator || !isMarkdownSeparator(separator)) continue;

    const rows = [];
    index += 2;
    while (index < lines.length) {
      const cells = splitMarkdownRow(lines[index]);
      if (!cells) break;
      if (cells.length === headers.length) {
        rows.push(Object.fromEntries(headers.map((header, cellIndex) => [header, cells[cellIndex] || ''])));
      }
      index += 1;
    }
    tables.push({ headers, rows });
  }
  return tables;
}

function findMarkdownTable(text, requiredHeaders) {
  return parseMarkdownTables(text).find((table) => (
    requiredHeaders.every((header) => table.headers.includes(header))
  ))?.rows || [];
}

function stripInlineCode(text) {
  return String(text || '').replace(/`([^`]+)`/g, '$1');
}

for (const relPath of requiredFiles) {
  if (!exists(relPath)) errors.push(`missing required file: ${relPath}`);
}

for (const deprecatedPath of [
  '.agents/skills/repo-wiki-maintainer',
  '.claude/skills/repo-wiki-maintainer',
]) {
  if (exists(deprecatedPath)) errors.push(`deprecated skill directory still exists: ${deprecatedPath}`);
}

const skillMirrorRequirements = {
  userwiki: [
    'SKILL.md',
    'references/install.md',
    'references/usage.md',
    'references/llm-wiki-principles.md',
    'references/qa-playbook.md',
    'references/qa-examples.md',
    'references/qa-smoke-cases.json',
    'references/maintenance-routing.md',
    'references/troubleshooting.md',
  ],
  'wiki-maintainer': [
    'SKILL.md',
    'references/source-policy.md',
    'references/enhancement-schema.md',
    'references/validation-rubric.md',
    'references/subagent-workflow.md',
  ],
};

for (const [skillName, files] of Object.entries(skillMirrorRequirements)) {
  for (const relPath of files) {
    const agentsPath = path.join(root, '.agents/skills', skillName, relPath);
    const claudePath = path.join(root, '.claude/skills', skillName, relPath);
    if (!fs.existsSync(agentsPath) || !fs.existsSync(claudePath)) continue;
    const agentsText = fs.readFileSync(agentsPath, 'utf8');
    const claudeText = fs.readFileSync(claudePath, 'utf8');
    if (agentsText !== claudeText) {
      errors.push(`${skillName} skill mirror mismatch: ${relPath}`);
    }
  }

  const skillPath = path.join(root, '.agents/skills', skillName, 'SKILL.md');
  if (!fs.existsSync(skillPath)) continue;
  const skillText = fs.readFileSync(skillPath, 'utf8');
  for (const reference of files.filter((file) => file.startsWith('references/'))) {
    if (!skillText.includes(reference)) {
      errors.push(`${skillName} SKILL.md does not mention ${reference}`);
    }
  }
}

if (exists('docs/README.md')) {
  const docsReadme = fs.readFileSync(path.join(root, 'docs/README.md'), 'utf8');
  if (!docsReadme.includes('repo-wiki/')) {
    errors.push('docs/README.md does not link repo-wiki/');
  }
}

if (exists('docs/repo-wiki/README.md')) {
  const repoWikiReadme = fs.readFileSync(path.join(root, 'docs/repo-wiki/README.md'), 'utf8');
  for (const relPath of ['coverage-manifest.md', 'writeback-queue.md']) {
    if (!repoWikiReadme.includes(relPath)) {
      errors.push(`docs/repo-wiki/README.md does not link ${relPath}`);
    }
  }
}

if (exists('docs/repo-wiki/index.md')) {
  const indexText = fs.readFileSync(path.join(root, 'docs/repo-wiki/index.md'), 'utf8');
  for (const relPath of ['coverage-manifest.md', 'writeback-queue.md']) {
    if (!indexText.includes(relPath)) {
      errors.push(`docs/repo-wiki/index.md does not link ${relPath}`);
    }
  }
}

if (exists('docs/repo-wiki/coverage-manifest.md')) {
  const coverageText = fs.readFileSync(path.join(root, 'docs/repo-wiki/coverage-manifest.md'), 'utf8');
  const coverageRows = findMarkdownTable(coverageText, ['Domain', 'Level', 'Evidence / Artifacts', 'Next Writeback']);
  const writebackText = exists('docs/repo-wiki/writeback-queue.md')
    ? fs.readFileSync(path.join(root, 'docs/repo-wiki/writeback-queue.md'), 'utf8')
    : '';
  const writebackRows = writebackText
    ? findMarkdownTable(writebackText, ['ID', 'Domain', 'Priority', 'State', 'Agent / Model', 'Expected Artifact', 'Close Criteria'])
    : [];
  if (coverageRows.length === 0) {
    errors.push('coverage-manifest.md missing High-Value Coverage table');
  }
  const coverageDomains = new Set();
  const allowedCoverageLevels = new Set(['strong', 'partial', 'queued', 'deferred']);
  for (const row of coverageRows) {
    if (!row.Domain) {
      errors.push('coverage-manifest.md has coverage row without Domain');
      continue;
    }
    if (coverageDomains.has(row.Domain)) {
      errors.push(`coverage-manifest.md duplicate Domain: ${row.Domain}`);
    }
    coverageDomains.add(row.Domain);
    if (!allowedCoverageLevels.has(row.Level)) {
      errors.push(`coverage-manifest.md invalid Level for ${row.Domain}: ${row.Level}`);
    }
    if (row.Level === 'queued') {
      const hasQueueItem = writebackRows.some((queueRow) => queueRow.Domain === row.Domain)
        || /WB-\d{4}-\d{2}-\d{2}-\d{3}/.test(row['Next Writeback']);
      if (!hasQueueItem) {
        errors.push(`coverage-manifest.md queued Domain has no matching writeback queue item: ${row.Domain}`);
      }
    }
    if (row.Level === 'strong') {
      const evidence = row['Evidence / Artifacts'];
      const hasEnhancementOrTooling = evidence.includes('.understand-anything/enhancements/')
        || evidence.includes('.agents/skills/')
        || evidence.includes('.claude/skills/')
        || evidence.includes('scripts/');
      const hasRepoWikiEntry = evidence.includes('docs/repo-wiki/');
      if (!hasEnhancementOrTooling) {
        errors.push(`coverage-manifest.md strong Domain lacks enhancement/tooling evidence: ${row.Domain}`);
      }
      if (!hasRepoWikiEntry) {
        errors.push(`coverage-manifest.md strong Domain lacks RepoWiki entry: ${row.Domain}`);
      }
    }
  }
  for (const required of [
    'Auth / user scope / account / billing boundary',
    'Prompt / context / compaction / cost accounting',
    'Tauri command / event contract surface',
    'test-intents / AEIT / `aijia` CLI',
    'Release / signing pipeline',
  ]) {
    if (!coverageText.includes(required)) {
      errors.push(`coverage-manifest.md missing tracked domain: ${required}`);
    }
  }
  for (const level of ['strong', 'partial', 'queued', 'deferred']) {
    if (!coverageText.includes(`| ${level} |`)) {
      errors.push(`coverage-manifest.md missing coverage level: ${level}`);
    }
  }
}

if (exists('docs/repo-wiki/writeback-queue.md')) {
  const writebackText = fs.readFileSync(path.join(root, 'docs/repo-wiki/writeback-queue.md'), 'utf8');
  const writebackRows = findMarkdownTable(writebackText, ['ID', 'Domain', 'Priority', 'State', 'Agent / Model', 'Expected Artifact', 'Close Criteria']);
  const coverageText = exists('docs/repo-wiki/coverage-manifest.md')
    ? fs.readFileSync(path.join(root, 'docs/repo-wiki/coverage-manifest.md'), 'utf8')
    : '';
  const coverageRows = coverageText
    ? findMarkdownTable(coverageText, ['Domain', 'Level', 'Evidence / Artifacts', 'Next Writeback'])
    : [];
  if (writebackRows.length === 0) {
    errors.push('writeback-queue.md missing Active Queue table');
  }
  const queueIds = new Set();
  const allowedPriorities = new Set(['P1', 'P2', 'P3']);
  const allowedStates = new Set(['candidate', 'agent-exploring', 'enhancement-draft', 'merged', 'validated', 'deferred']);
  for (const row of writebackRows) {
    if (!/^WB-\d{4}-\d{2}-\d{2}-\d{3}$/.test(row.ID)) {
      errors.push(`writeback-queue.md invalid ID: ${row.ID}`);
    }
    if (queueIds.has(row.ID)) {
      errors.push(`writeback-queue.md duplicate ID: ${row.ID}`);
    }
    queueIds.add(row.ID);
    if (!allowedPriorities.has(row.Priority)) {
      errors.push(`writeback-queue.md invalid Priority for ${row.ID}: ${row.Priority}`);
    }
    if (!allowedStates.has(row.State)) {
      errors.push(`writeback-queue.md invalid State for ${row.ID}: ${row.State}`);
    }
    if (coverageRows.length > 0 && !coverageRows.some((coverageRow) => coverageRow.Domain === row.Domain)) {
      errors.push(`writeback-queue.md Domain is not tracked in coverage manifest: ${row.Domain}`);
    }
    const expectedArtifact = stripInlineCode(row['Expected Artifact']);
    const enhancementMatch = expectedArtifact.match(/\.understand-anything\/enhancements\/[^\s,]+\.json/);
    if (enhancementMatch) {
      const enhancementPath = enhancementMatch[0];
      if (!/^\.understand-anything\/enhancements\/[a-z0-9]+(?:-[a-z0-9]+)*\.json$/.test(enhancementPath)) {
        errors.push(`writeback-queue.md Expected Artifact is not kebab-case enhancement path: ${enhancementPath}`);
      }
      if ((row.State === 'merged' || row.State === 'validated') && !exists(enhancementPath)) {
        errors.push(`writeback-queue.md ${row.State} item references missing enhancement: ${enhancementPath}`);
      }
    }
    if (row.State === 'validated') {
      const coverageRow = coverageRows.find((candidate) => candidate.Domain === row.Domain);
      if (coverageRow?.Level === 'queued') {
        errors.push(`writeback-queue.md validated item still has queued coverage: ${row.Domain}`);
      }
    }
  }
  for (const id of [
    'WB-2026-06-04-001',
    'WB-2026-06-04-002',
    'WB-2026-06-04-003',
    'WB-2026-06-04-004',
    'WB-2026-06-04-005',
  ]) {
    if (!writebackText.includes(id)) {
      errors.push(`writeback-queue.md missing queue item: ${id}`);
    }
  }
  for (const state of ['candidate', 'agent-exploring', 'enhancement-draft', 'merged', 'validated', 'deferred']) {
    if (!writebackText.includes(`| ${state} |`)) {
      errors.push(`writeback-queue.md missing state: ${state}`);
    }
  }
}

let graphStats = null;

if (exists('.understand-anything/config.json')) {
  const config = JSON.parse(fs.readFileSync(path.join(root, '.understand-anything/config.json'), 'utf8'));
  if (config.outputLanguage !== 'zh') {
    errors.push(`expected .understand-anything/config.json outputLanguage=zh, got ${JSON.stringify(config.outputLanguage)}`);
  }
}

if (exists('.understand-anything/knowledge-graph.json')) {
  const graph = JSON.parse(fs.readFileSync(path.join(root, '.understand-anything/knowledge-graph.json'), 'utf8'));
  for (const key of ['project', 'nodes', 'edges', 'layers', 'tour']) {
    if (!(key in graph)) errors.push(`knowledge graph missing key: ${key}`);
  }
  if (!Array.isArray(graph.nodes) || graph.nodes.length === 0) errors.push('knowledge graph has no nodes');
  if (!Array.isArray(graph.edges) || graph.edges.length === 0) errors.push('knowledge graph has no edges');
  if (!Array.isArray(graph.layers) || graph.layers.length === 0) errors.push('knowledge graph has no layers');
  if (!Array.isArray(graph.tour) || graph.tour.length === 0) errors.push('knowledge graph has no tour');
  graphStats = {
    nodes: graph.nodes.length,
    edges: graph.edges.length,
    layers: graph.layers.length,
    tour: graph.tour.length,
    llmEnhanced: (graph.nodes || []).filter((node) => node.llmEnhanced).length,
    architectureReview: (graph.nodes || []).filter((node) => (
      node.type === 'concept' && String(node.id || '').startsWith('concept:architecture-review:')
    )).length,
    layerSizes: Object.fromEntries((graph.layers || []).map((layer) => [layer.name, (layer.nodeIds || []).length])),
  };
  const staleSkillNodes = (graph.nodes || []).filter((node) => String(node.filePath || '').includes('repo-wiki-maintainer'));
  if (staleSkillNodes.length > 0) {
    errors.push(`knowledge graph still references deprecated repo-wiki-maintainer paths: ${staleSkillNodes.length}`);
  }
}

if (exists('.agents/skills/userwiki/references/qa-smoke-cases.json')) {
  const cases = JSON.parse(fs.readFileSync(path.join(root, '.agents/skills/userwiki/references/qa-smoke-cases.json'), 'utf8'));
  if (!Array.isArray(cases) || cases.length < 5) {
    errors.push('expected at least 5 userwiki QA smoke cases');
  } else {
    const ids = new Set();
    for (const testCase of cases) {
      if (!testCase?.id || !/^[a-z0-9-]+$/.test(testCase.id)) {
        errors.push(`invalid userwiki QA case id: ${JSON.stringify(testCase?.id)}`);
      }
      if (ids.has(testCase.id)) errors.push(`duplicate userwiki QA case id: ${testCase.id}`);
      ids.add(testCase.id);
      const question = String(testCase?.question || '');
      if (!question.includes('userwiki') && !question.includes('wiki')) {
        errors.push(`userwiki QA case ${testCase.id || '<missing>'} must include a userwiki-triggering question`);
      }
      if (!Array.isArray(testCase.requiredTerms) || testCase.requiredTerms.length === 0) {
        errors.push(`userwiki QA case ${testCase.id || '<missing>'} missing requiredTerms`);
      }
      if (testCase.requiredAnyTerms && (
        !Array.isArray(testCase.requiredAnyTerms)
        || testCase.requiredAnyTerms.some((group) => !Array.isArray(group) || group.length === 0)
      )) {
        errors.push(`userwiki QA case ${testCase.id || '<missing>'} has invalid requiredAnyTerms`);
      }
    }
  }
}

const enhancementsDir = path.join(root, '.understand-anything/enhancements');
if (fs.existsSync(enhancementsDir)) {
  const enhancementFiles = fs.readdirSync(enhancementsDir)
    .filter((name) => name.endsWith('.json'))
    .sort();

  if (enhancementFiles.length < 3) {
    errors.push(`expected at least 3 current-source graph enhancement files, got ${enhancementFiles.length}`);
  }

  for (const name of enhancementFiles) {
    const relPath = `.understand-anything/enhancements/${name}`;
    const enhancement = JSON.parse(fs.readFileSync(path.join(root, relPath), 'utf8'));
    if (!enhancement.module) errors.push(`${relPath} missing module`);
    if (String(enhancement.module || '').includes('docs-tests-release')) {
      errors.push(`${relPath} is docs-only governance output; graph enhancements must be current-source sourced`);
    }
    for (const key of ['key_nodes', 'semantic_edges', 'architecture_findings', 'tour_steps']) {
      if (!Array.isArray(enhancement[key]) || enhancement[key].length === 0) {
        errors.push(`${relPath} missing non-empty ${key}`);
      }
    }
    const currentSourceNodes = (enhancement.key_nodes || []).filter((node) => {
      if (!node?.filePath) return false;
      const sourcePath = String(node.filePath);
      return !sourcePath.startsWith('docs/') && sourcePath !== 'AGENTS.md' && sourcePath !== 'CLAUDE.md';
    });
    if (currentSourceNodes.length === 0) {
      errors.push(`${relPath} has no current-source key_nodes`);
    }
    for (const node of enhancement.key_nodes || []) {
      if (!node?.filePath || !node?.summary || !Array.isArray(node.tags) || !node.complexity) {
        errors.push(`${relPath} has malformed key_node`);
        continue;
      }
      if (!fs.existsSync(path.join(root, node.filePath))) {
        errors.push(`${relPath} references missing key_node file: ${node.filePath}`);
      }
    }
    for (const edge of enhancement.semantic_edges || []) {
      if (!edge?.sourceFilePath || !edge?.targetFilePath || !edge?.type || !edge?.reason) {
        errors.push(`${relPath} has malformed semantic_edge`);
        continue;
      }
      if (!fs.existsSync(path.join(root, edge.sourceFilePath))) {
        errors.push(`${relPath} references missing semantic_edge source: ${edge.sourceFilePath}`);
      }
      if (!fs.existsSync(path.join(root, edge.targetFilePath))) {
        errors.push(`${relPath} references missing semantic_edge target: ${edge.targetFilePath}`);
      }
    }
  }

  if (graphStats && exists('docs/repo-wiki/index.md')) {
    const indexText = fs.readFileSync(path.join(root, 'docs/repo-wiki/index.md'), 'utf8');
    const expectedSnippets = [
      `${graphStats.nodes} 个节点`,
      `${graphStats.edges} 条边`,
      `${graphStats.layers} 个 architecture layers`,
      `${graphStats.tour} 个 guided tour steps`,
      `${graphStats.llmEnhanced} 个 LLM-enhanced 节点`,
      `${graphStats.architectureReview} 个代码/维护架构评审概念节点`,
      `${enhancementFiles.length} 份当前源码/测试/skill 来源 enhancement JSON`,
    ];
    for (const snippet of expectedSnippets) {
      if (!indexText.includes(snippet)) {
        errors.push(`docs/repo-wiki/index.md missing current graph stat: ${snippet}`);
      }
    }
    const indexLayerSizes = {};
    for (const line of indexText.split(/\r?\n/)) {
      if (!line.startsWith('| ') || line.includes('---')) continue;
      const cells = line.split('|').slice(1, -1).map((cell) => cell.trim());
      if (cells.length !== 3 || cells[0] === 'Layer') continue;
      const size = Number(cells[2]);
      if (Number.isFinite(size)) indexLayerSizes[cells[0]] = size;
    }
    for (const [layerName, size] of Object.entries(graphStats.layerSizes)) {
      if (indexLayerSizes[layerName] !== size) {
        errors.push(`docs/repo-wiki/index.md layer size drift: ${layerName} expected ${size}, got ${indexLayerSizes[layerName] ?? '<missing>'}`);
      }
    }
  }
}

const wikiDir = path.join(root, 'docs/repo-wiki');
const linkPattern = /!?\[[^\]]*\]\(([^)]+)\)/g;
if (fs.existsSync(wikiDir)) {
  for (const name of fs.readdirSync(wikiDir)) {
    if (!name.endsWith('.md')) continue;
    const relFile = `docs/repo-wiki/${name}`;
    const absFile = path.join(root, relFile);
    const text = fs.readFileSync(absFile, 'utf8');
    for (const match of text.matchAll(linkPattern)) {
      const rawTarget = match[1].trim();
      if (!rawTarget || rawTarget.startsWith('http://') || rawTarget.startsWith('https://') || rawTarget.startsWith('mailto:')) {
        continue;
      }
      const withoutFragment = rawTarget.split('#')[0];
      if (!withoutFragment) continue;
      const normalized = withoutFragment.replace(/^<|>$/g, '');
      const targetAbs = path.resolve(path.dirname(absFile), normalized);
      if (!fs.existsSync(targetAbs)) {
        errors.push(`${relFile} has broken local link: ${rawTarget}`);
      }
    }
  }
}

if (errors.length) {
  console.error(`RepoWiki validation failed (${errors.length} issue${errors.length === 1 ? '' : 's'}):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

console.log('RepoWiki validation passed.');
