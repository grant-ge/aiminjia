import { describe, it, expect } from 'vitest'

import { classifyToolBucket, summarizeToolSteps } from '../toolStepSummary'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

function step(name: string, status: RenderToolStep['status'] = 'done'): RenderToolStep {
  return { index: 0, toolCallId: name + Math.random(), name, status }
}

describe('classifyToolBucket', () => {
  it('Bash / shell → command', () => {
    expect(classifyToolBucket('Bash')).toBe('command')
    expect(classifyToolBucket('shell_run')).toBe('command')
    expect(classifyToolBucket('shell')).toBe('command')
  })
  it('Read → file_read', () => {
    expect(classifyToolBucket('Read')).toBe('file_read')
    expect(classifyToolBucket('read_file')).toBe('file_read')
  })
  it('Write/Edit → file_edit', () => {
    expect(classifyToolBucket('Write')).toBe('file_edit')
    expect(classifyToolBucket('Edit')).toBe('file_edit')
    expect(classifyToolBucket('MultiEdit')).toBe('file_edit')
  })
  it('Grep/Glob → search', () => {
    expect(classifyToolBucket('Grep')).toBe('search')
    expect(classifyToolBucket('Glob')).toBe('search')
  })
  it('mcp__* → mcp', () => {
    expect(classifyToolBucket('mcp__pencil__batch_get')).toBe('mcp')
  })
  it('unknown → other', () => {
    expect(classifyToolBucket('FancyTool')).toBe('other')
  })
})

describe('summarizeToolSteps', () => {
  it('按出现顺序聚合 bucket', () => {
    const steps = [step('Read'), step('Read'), step('Bash'), step('Read'), step('Bash')]
    const r = summarizeToolSteps(steps)
    expect(r.buckets).toEqual([
      { key: 'file_read', count: 3 },
      { key: 'command', count: 2 },
    ])
  })

  it('统计 running / error', () => {
    const steps = [step('Read', 'running'), step('Bash', 'error'), step('Read', 'done')]
    const r = summarizeToolSteps(steps)
    expect(r.runningCount).toBe(1)
    expect(r.errorCount).toBe(1)
  })

  it('空列表 → buckets 空', () => {
    expect(summarizeToolSteps([])).toEqual({ buckets: [], runningCount: 0, errorCount: 0 })
  })
})
