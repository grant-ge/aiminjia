import '@testing-library/jest-dom'
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'

import { ToolStepGroupBlock } from '../ToolStepGroupBlock'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

function step(name: string, status: RenderToolStep['status'] = 'done', id?: string): RenderToolStep {
  return { index: 0, toolCallId: id ?? name + Math.random(), name, status }
}

describe('ToolStepGroupBlock — 折叠态', () => {
  it('单个 Read → "读取了 1 个文件"', () => {
    render(<ToolStepGroupBlock steps={[step('Read')]} />)
    expect(screen.getByText(/读取了 1 个文件/)).toBeInTheDocument()
  })

  it('3 Read + 2 Bash → "读取了 3 个文件、运行了 2 个命令"', () => {
    const steps = [step('Read'), step('Read'), step('Read'), step('Bash'), step('Bash')]
    render(<ToolStepGroupBlock steps={steps} />)
    expect(screen.getByText(/读取了 3 个文件、运行了 2 个命令/)).toBeInTheDocument()
  })

  it('包含 running → 显示 spinner 和 runningSuffix …', () => {
    const steps = [step('Read', 'running'), step('Read', 'done')]
    const { container } = render(<ToolStepGroupBlock steps={steps} />)
    expect(container.querySelector('.animate-spin')).toBeTruthy()
    expect(screen.getByText(/读取了 2 个文件…/)).toBeInTheDocument()
  })

  it('包含 error → 显示 "1 个失败"', () => {
    const steps = [step('Read', 'done'), step('Bash', 'error')]
    render(<ToolStepGroupBlock steps={steps} />)
    expect(screen.getByText(/1 个失败/)).toBeInTheDocument()
  })

  it('折叠态默认不渲染 ToolStepRow', () => {
    const steps = [step('Read', 'done', 'tc-1'), step('Bash', 'done', 'tc-2')]
    const { container } = render(<ToolStepGroupBlock steps={steps} />)
    expect(within(container).queryByText('⎿')).not.toBeInTheDocument()
  })
})

describe('ToolStepGroupBlock — 一级展开', () => {
  it('点击摘要行 → 展开 N 个 ToolStepRow', () => {
    const steps = [
      step('Read', 'done', 'tc-1'),
      step('Bash', 'done', 'tc-2'),
    ]
    render(<ToolStepGroupBlock steps={steps} />)
    fireEvent.click(screen.getByRole('button'))
    const rows = screen.getAllByText('⎿')
    expect(rows).toHaveLength(2)
  })
})
