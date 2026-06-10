#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const graphPath = path.join(root, '.understand-anything/knowledge-graph.json');
const enhancementsDir = path.join(root, '.understand-anything/enhancements');

const allowedEdgeTypes = new Set([
  'imports', 'exports', 'contains', 'inherits', 'implements',
  'calls', 'subscribes', 'publishes', 'middleware',
  'reads_from', 'writes_to', 'transforms', 'validates',
  'depends_on', 'tested_by', 'configures',
  'related', 'similar_to',
  'deploys', 'serves', 'provisions', 'triggers',
  'migrates', 'documents', 'routes', 'defines_schema',
  'contains_flow', 'flow_step', 'cross_domain',
  'cites', 'contradicts', 'builds_on', 'exemplifies',
  'categorized_under', 'authored_by',
]);

const edgeTypeAliases = new Map([
  ['uses', 'depends_on'],
  ['describes', 'documents'],
  ['references', 'cites'],
  ['relates_to', 'related'],
  ['related_to', 'related'],
  ['contained', 'contains'],
  ['orchestrates', 'calls'],
  ['renders', 'calls'],
  ['ipc', 'calls'],
  ['ownership-lookup', 'reads_from'],
  ['physical-file-access', 'calls'],
  ['authorization-persist', 'writes_to'],
  ['conversation-binding', 'writes_to'],
  ['context-injection', 'configures'],
  ['permission-load', 'reads_from'],
  ['snapshot-bridge', 'transforms'],
  ['authorization-check', 'validates'],
  ['grant-persist', 'writes_to'],
  ['session-working-dirs', 'configures'],
  ['user-scope-resolution', 'configures'],
  ['workspace-root-refresh', 'configures'],
  ['authority-reference', 'documents'],
  ['directory-entry', 'documents'],
  ['required-page-link', 'documents'],
  ['maintenance-skill-link', 'documents'],
  ['policy-alignment', 'documents'],
  ['required-validation', 'validates'],
  ['validation-target', 'validates'],
  ['verification-handoff', 'documents'],
  ['entry-to-skill', 'documents'],
  ['routing', 'routes'],
  ['single-source-of-gap', 'documents'],
  ['command-contract', 'documents'],
  ['feature-contract', 'documents'],
]);

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function writeJson(filePath, value) {
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

function slug(value) {
  return String(value ?? '')
    .toLowerCase()
    .replace(/[^a-z0-9\u4e00-\u9fa5]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80) || 'item';
}

function normalizeComplexity(value) {
  const normalized = String(value ?? '').toLowerCase();
  if (normalized === 'low' || normalized === 'simple') return 'simple';
  if (normalized === 'medium' || normalized === 'moderate') return 'moderate';
  if (normalized === 'high' || normalized === 'critical' || normalized === 'complex') return 'complex';
  return 'moderate';
}

function normalizeEdgeType(value) {
  const raw = String(value ?? 'related').toLowerCase();
  const aliased = edgeTypeAliases.get(raw) ?? raw;
  return allowedEdgeTypes.has(aliased) ? aliased : 'related';
}

function clampWeight(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return 0.7;
  return Math.max(0, Math.min(1, number));
}

function nodeIdForFilePath(graph, filePath) {
  const node = graph.nodes.find((candidate) => candidate.filePath === filePath);
  return node?.id;
}

function ensureFileNode(graph, filePath, seed = {}) {
  const existing = nodeIdForFilePath(graph, filePath);
  if (existing) return existing;

  const type = filePath.endsWith('.md') ? 'document' : 'file';
  const id = `${type}:${filePath}`;
  graph.nodes.push({
    id,
    type,
    name: path.basename(filePath),
    filePath,
    summary: seed.summary || '由 Understand-Anything 增强流程补充的项目文件节点。',
    tags: Array.from(new Set([...(seed.tags || []), 'ua-enhanced'])),
    complexity: normalizeComplexity(seed.complexity),
    languageNotes: '由子 agent 增强材料补入图谱。',
  });
  return id;
}

function mergeTags(current, extra) {
  return Array.from(new Set([...(current || []), ...extra.filter(Boolean)]));
}

function upsertEdge(graph, edge) {
  const key = `${edge.source}|${edge.target}|${edge.type}|${edge.description ?? ''}`;
  const existing = graph.edges.find((candidate) => (
    `${candidate.source}|${candidate.target}|${candidate.type}|${candidate.description ?? ''}` === key
  ));
  if (existing) {
    existing.weight = Math.max(existing.weight, edge.weight);
    return false;
  }
  graph.edges.push(edge);
  return true;
}

function addLayerNode(graph, layerId, nodeId) {
  const layer = graph.layers.find((candidate) => candidate.id === layerId);
  if (layer && !layer.nodeIds.includes(nodeId)) layer.nodeIds.push(nodeId);
}

function ensureLayer(graph, layer) {
  const existing = graph.layers.find((candidate) => candidate.id === layer.id);
  if (existing) return existing;
  graph.layers.push({ ...layer, nodeIds: [] });
  return graph.layers[graph.layers.length - 1];
}

function loadEnhancements() {
  if (!fs.existsSync(enhancementsDir)) return [];
  return fs.readdirSync(enhancementsDir)
    .filter((name) => name.endsWith('.json'))
    .sort()
    .flatMap((name) => {
      const absPath = path.join(enhancementsDir, name);
      const parsed = readJson(absPath);
      const items = Array.isArray(parsed) ? parsed : [parsed];
      return items.map((item) => ({ ...item, __sourceFile: path.relative(root, absPath) }));
    });
}

if (!fs.existsSync(graphPath)) {
  console.error('missing .understand-anything/knowledge-graph.json');
  process.exit(1);
}

const graph = readJson(graphPath);
const enhancements = loadEnhancements();
const now = new Date().toISOString();

let updatedNodes = 0;
let addedNodes = 0;
let addedEdges = 0;
let addedTourSteps = 0;
const modules = [];

for (const enhancement of enhancements) {
  const moduleName = enhancement.module || path.basename(enhancement.__sourceFile, '.json');
  modules.push(moduleName);

  for (const item of enhancement.key_nodes || []) {
    if (!item.filePath) continue;
    const beforeCount = graph.nodes.length;
    const nodeId = ensureFileNode(graph, item.filePath, item);
    if (graph.nodes.length > beforeCount) addedNodes += 1;

    const node = graph.nodes.find((candidate) => candidate.id === nodeId);
    if (!node) continue;
    if (item.summary) node.summary = item.summary;
    node.tags = mergeTags(node.tags, ['llm-enhanced', 'ua-enhanced', moduleName, ...(item.tags || [])]);
    node.complexity = normalizeComplexity(item.complexity || node.complexity);
    const priorNotes = node.languageNotes ? `${node.languageNotes}\n` : '';
    node.languageNotes = `${priorNotes}增强来源：${moduleName}；${enhancement.__sourceFile}`.trim();
    node.llmEnhanced = true;
    node.llmEnhancedModule = moduleName;
    node.llmEnhancedAt = now;
    updatedNodes += 1;
  }

  for (const relation of enhancement.semantic_edges || []) {
    if (!relation.sourceFilePath || !relation.targetFilePath) continue;
    const source = ensureFileNode(graph, relation.sourceFilePath);
    const target = ensureFileNode(graph, relation.targetFilePath);
    if (source === target) continue;
    const inserted = upsertEdge(graph, {
      source,
      target,
      type: normalizeEdgeType(relation.type),
      direction: 'forward',
      weight: clampWeight(relation.weight),
      description: relation.reason || `由 ${moduleName} 子 agent 补充的语义关系。`,
    });
    if (inserted) addedEdges += 1;
  }

  for (const rawFinding of enhancement.architecture_findings || []) {
    const finding = typeof rawFinding === 'string'
      ? { title: rawFinding.slice(0, 80), description: rawFinding, evidence: [] }
      : rawFinding;
    if (!finding?.title) continue;
    const id = `concept:architecture-review:${moduleName}:${slug(finding.title)}`;
    if (!graph.nodes.some((node) => node.id === id)) {
      graph.nodes.push({
        id,
        type: 'concept',
        name: finding.title,
        summary: finding.description || finding.title,
        tags: ['architecture-review', 'llm-enhanced', 'ua-enhanced', moduleName],
        complexity: 'moderate',
        languageNotes: `增强来源：${moduleName}；${enhancement.__sourceFile}`,
        knowledgeMeta: {
          category: 'architecture-review',
          evidence: finding.evidence || [],
        },
        llmEnhanced: true,
        llmEnhancedModule: moduleName,
        llmEnhancedAt: now,
      });
      addedNodes += 1;
      ensureLayer(graph, {
        id: 'layer:architecture-review',
        name: '代码架构评审',
        description: '由代码/测试来源的子 agent 增强材料生成的架构评审概念节点。',
      });
      addLayerNode(graph, 'layer:architecture-review', id);
    }

    for (const evidence of finding.evidence || []) {
      const evidencePath = String(evidence).split(':').slice(0, -1).join(':') || String(evidence);
      if (!evidencePath || !fs.existsSync(path.join(root, evidencePath))) continue;
      const target = ensureFileNode(graph, evidencePath);
      const inserted = upsertEdge(graph, {
        source: id,
        target,
        type: 'related',
        direction: 'forward',
        weight: 0.7,
        description: `架构评审证据：${finding.title}`,
      });
      if (inserted) addedEdges += 1;
    }
  }

  for (const step of enhancement.tour_steps || []) {
    if (!step.title) continue;
    const nodeIds = (step.filePaths || [])
      .map((filePath) => nodeIdForFilePath(graph, filePath))
      .filter(Boolean);
    if (nodeIds.length === 0) continue;
    const existing = graph.tour.find((candidate) => candidate.title === step.title);
    if (existing) {
      existing.description = step.description || existing.description;
      existing.nodeIds = Array.from(new Set([...existing.nodeIds, ...nodeIds]));
      existing.languageLesson = existing.languageLesson || `模块：${moduleName}`;
      continue;
    }
    graph.tour.push({
      order: graph.tour.length + 1,
      title: step.title,
      description: step.description || step.title,
      nodeIds,
      languageLesson: `模块：${moduleName}`,
    });
    addedTourSteps += 1;
  }
}

graph.project = {
  ...graph.project,
  description: `${graph.project.description || ''}`.includes('全库知识图谱增强')
    ? graph.project.description
    : `${graph.project.description || ''} 已补入全库知识图谱增强材料。`.trim(),
  llmEnhancedAt: now,
  llmEnhancementScope: Array.from(new Set([...(graph.project.llmEnhancementScope || []), ...modules])),
  graphEnhancementFiles: enhancements.map((item) => item.__sourceFile),
};

graph.tour.forEach((step, index) => {
  step.order = index + 1;
});

writeJson(graphPath, graph);

console.log(JSON.stringify({
  enhancementFiles: enhancements.length,
  modules,
  updatedNodes,
  addedNodes,
  addedEdges,
  addedTourSteps,
  totalNodes: graph.nodes.length,
  totalEdges: graph.edges.length,
  totalTourSteps: graph.tour.length,
}, null, 2));
