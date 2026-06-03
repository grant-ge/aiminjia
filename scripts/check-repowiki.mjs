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
  'docs/repo-wiki/log.md',
  '.agents/skills/userwiki/SKILL.md',
  '.agents/skills/userwiki/references/install.md',
  '.agents/skills/userwiki/references/usage.md',
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
