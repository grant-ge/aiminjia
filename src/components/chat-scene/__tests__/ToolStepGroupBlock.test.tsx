import '@testing-library/jest-dom'
import { beforeEach, describe, it, expect } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'

import { ToolStepGroupBlock } from '../ToolStepGroupBlock'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'
import { useDevSettingsStore } from '@/stores/devSettingsStore'

function step(name: string, status: RenderToolStep['status'] = 'done', id?: string): RenderToolStep {
  return { index: 0, toolCallId: id ?? name + Math.random(), name, status }
}

describe('ToolStepGroupBlock — 折叠态', () => {
  beforeEach(() => {
    useDevSettingsStore.setState({ showToolErrorIcon: false })
  })

  it('单个 Read → "读取了 1 个文件"', () => {
    render(<ToolStepGroupBlock steps={[step('Read')]} />)
    expect(screen.getByText(/读取了 1 个文件/)).toBeInTheDocument()
  })

  it('3 Read + 2 Bash → "读取了 3 个文件、运行了 2 个命令"', () => {
    const steps = [step('Read'), step('Read'), step('Read'), step('Bash'), step('Bash')]
    render(<ToolStepGroupBlock steps={steps} />)
    expect(screen.getByRole('button', { name: /读取了 3 个文件、运行了 2 个命令/ })).toBeInTheDocument()
  })

  it('按工具类型显示摘要图标', () => {
    const steps = [step('Read'), step('Bash')]
    render(<ToolStepGroupBlock steps={steps} />)

    expect(screen.getByTestId('tool-bucket-icon-file_read')).toBeInTheDocument()
    expect(screen.getByTestId('tool-bucket-icon-command')).toBeInTheDocument()
  })

  it('包含 running → 显示 spinner 和 runningSuffix …', () => {
    const steps = [step('Read', 'running'), step('Read', 'done')]
    const { container } = render(<ToolStepGroupBlock steps={steps} />)
    expect(container.querySelector('.animate-spin')).toBeTruthy()
    expect(screen.getByRole('button', { name: /读取了 2 个文件…/ })).toBeInTheDocument()
  })

  it('dev 开关开启时，包含 error → 显示 "1 个失败" 和红色失败 icon', () => {
    useDevSettingsStore.setState({ showToolErrorIcon: true })

    const steps = [step('Read', 'done'), step('Bash', 'error')]
    render(<ToolStepGroupBlock steps={steps} />)
    expect(screen.getByText(/1 个失败/)).toBeInTheDocument()
    expect(screen.getByTestId('tool-step-error-icon')).toBeInTheDocument()
  })

  it('dev 开关默认关闭时，隐藏失败数量和红色失败 icon', () => {
    useDevSettingsStore.setState({ showToolErrorIcon: false })

    const steps = [step('Read', 'done'), step('Bash', 'error')]
    render(<ToolStepGroupBlock steps={steps} />)

    expect(screen.queryByText(/1 个失败/)).not.toBeInTheDocument()
    expect(screen.queryByTestId('tool-step-error-icon')).not.toBeInTheDocument()
  })

  it('dev 开关默认关闭时，展开后也隐藏失败行里的红色 icon', () => {
    const steps = [step('Read', 'done', 'tc-1'), step('Bash', 'error', 'tc-2')]
    render(<ToolStepGroupBlock steps={steps} />)

    fireEvent.click(screen.getByRole('button'))

    expect(screen.getByText('Bash')).toBeInTheDocument()
    expect(screen.queryByTestId('tool-step-row-error-icon')).not.toBeInTheDocument()
  })

  it('只有 AskUserQuestion 时渲染用户交互聚合条', () => {
    render(
      <ToolStepGroupBlock
        steps={[
          {
            ...step('AskUserQuestion', 'done', 'ask-1'),
            inputJson: JSON.stringify({
              questions: [
                { question: '任务类型？' },
                { question: '输出格式？' },
                { question: '优先级？' },
              ],
            }),
          },
        ]}
      />,
    )

    expect(screen.getByRole('button', { name: /询问了用户 3 个问题/ })).toBeInTheDocument()
  })

  it('AskUserQuestion 聚合条下方显示收到的用户回答摘要', () => {
    render(
      <ToolStepGroupBlock
        steps={[
          {
            ...step('AskUserQuestion', 'done', 'ask-1'),
            inputJson: JSON.stringify({
              questions: [
                { question: '工作方向？' },
                { question: '业务领域？' },
                { question: '紧急程度？' },
              ],
            }),
            output: 'User has answered your questions: "工作方向？"="数据分析", "业务领域？"="人力/薪酬", "紧急程度？"="今天就要". You can now continue with the user\'s answers in mind.',
          },
        ]}
      />,
    )

    expect(screen.getByRole('button', { name: /询问了用户 3 个问题/ })).toBeInTheDocument()
    expect(screen.getByText('收到：数据分析 / 人力/薪酬 / 今天就要')).toBeInTheDocument()
    expect(screen.queryByText('输入')).not.toBeInTheDocument()
  })

  it('AskUserQuestion 展开后先显示工具详情，再显示收到的用户回答摘要', () => {
    render(
      <ToolStepGroupBlock
        steps={[
          {
            ...step('AskUserQuestion', 'done', 'ask-1'),
            inputJson: JSON.stringify({
              questions: [
                { question: '工作方向？' },
                { question: '业务领域？' },
                { question: '紧急程度？' },
              ],
            }),
            output: 'User has answered your questions: "工作方向？"="数据分析", "业务领域？"="人力/薪酬", "紧急程度？"="今天就要". You can now continue with the user\'s answers in mind.',
          },
        ]}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: /询问了用户 3 个问题/ }))

    const toolRow = screen.getByText('AskUserQuestion')
    const receipt = screen.getByText('收到：数据分析 / 人力/薪酬 / 今天就要')
    expect(toolRow.compareDocumentPosition(receipt) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('折叠态默认不渲染 ToolStepRow', () => {
    const steps = [step('Read', 'done', 'tc-1'), step('Bash', 'done', 'tc-2')]
    const { container } = render(<ToolStepGroupBlock steps={steps} />)
    expect(within(container).queryByText('Read')).not.toBeInTheDocument()
    expect(within(container).queryByText('Bash')).not.toBeInTheDocument()
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
    expect(screen.getByText('Read')).toBeInTheDocument()
    expect(screen.getByText('Bash')).toBeInTheDocument()
  })

})
