import '@testing-library/jest-dom'
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import { ToolStepRow } from '../ToolStepRow'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

function makeStep(overrides: Partial<RenderToolStep> = {}): RenderToolStep {
  return {
    index: 0,
    toolCallId: 't1',
    name: 'Read',
    status: 'done',
    inputJson: JSON.stringify({ file_path: '/foo/bar/base_prompt.rs' }),
    output: 'file contents',
    ...overrides,
  }
}

describe('ToolStepRow', () => {
  it('Read 工具：显示 basename', () => {
    render(<ToolStepRow step={makeStep()} />)
    expect(screen.getByText(/Read/)).toBeInTheDocument()
    expect(screen.getByText(/base_prompt\.rs/)).toBeInTheDocument()
  })

  it('Bash 工具：显示 command 截断', () => {
    const step = makeStep({
      name: 'Bash',
      inputJson: JSON.stringify({ command: 'ls -la /tmp/foo' }),
    })
    render(<ToolStepRow step={step} />)
    expect(screen.getByText(/Bash/)).toBeInTheDocument()
    expect(screen.getByText(/ls -la \/tmp\/foo/)).toBeInTheDocument()
  })

  it('未知工具名：只显示 tool name', () => {
    const step = makeStep({ name: 'Weird', inputJson: undefined })
    render(<ToolStepRow step={step} />)
    expect(screen.getByText('Weird')).toBeInTheDocument()
  })

  it('inputJson 不合法：fallback 只显示 tool name', () => {
    const step = makeStep({ name: 'Read', inputJson: 'not json' })
    render(<ToolStepRow step={step} />)
    expect(screen.getByText('Read')).toBeInTheDocument()
  })

  it('点击行 toggle 二级展开（渲染 ToolTraceIO output）', () => {
    render(<ToolStepRow step={makeStep()} />)
    expect(screen.queryByText('file contents')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('file contents')).toBeInTheDocument()
  })

  it('running + progressTail 时自动展开', () => {
    const step = makeStep({ status: 'running', output: undefined, progressTail: 'tail line' })
    render(<ToolStepRow step={step} />)
    expect(screen.getByText('tail line')).toBeInTheDocument()
  })
})
