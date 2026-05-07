import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ExecutionTraceCard } from './ExecutionTraceCard'

describe('ExecutionTraceCard', () => {
  it('renders header, optional summary, sections and lazy expanded content', () => {
    const onExpand = vi.fn(() => <div>expanded body</div>)

    render(
      <ExecutionTraceCard
        title="子代理结果"
        badge="4 轮迭代"
        summary="分析完成"
        sections={[
          { title: '生成文件', items: ['report.xlsx', 'chart.png'] },
        ]}
        expandLabel="查看执行轨迹"
        collapseLabel="收起执行轨迹"
        expandedContent={onExpand}
      />,
    )

    expect(screen.getByText('子代理结果')).toBeInTheDocument()
    expect(screen.getByText('4 轮迭代')).toBeInTheDocument()
    expect(screen.getByText('分析完成')).toBeInTheDocument()
    expect(screen.getByText('生成文件')).toBeInTheDocument()
    expect(screen.getByText('report.xlsx')).toBeInTheDocument()
    expect(screen.queryByText('expanded body')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /查看执行轨迹/ }))

    expect(screen.getByText('expanded body')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /收起执行轨迹/ })).toBeInTheDocument()
    expect(onExpand).toHaveBeenCalledTimes(1)
  })

  it('omits summary, sections and expander when they are not provided', () => {
    render(<ExecutionTraceCard title="工具执行轨迹" badge="已完成 2 步" />)

    expect(screen.getByText('工具执行轨迹')).toBeInTheDocument()
    expect(screen.getByText('已完成 2 步')).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('does not add a top border to the first section when there is no summary', () => {
    const { container } = render(
      <ExecutionTraceCard
        title="工具执行轨迹"
        badge="已完成 2 步"
        sections={[{ title: '工具步骤', items: ['Read'] }]}
        expandLabel="查看执行详情"
        expandedContent={<div>details</div>}
      />,
    )

    const section = screen.getByText('工具步骤').closest('div')?.parentElement
    expect(section).not.toHaveClass('border-t')
    expect(container.querySelectorAll('.border-t')).toHaveLength(1)
  })


  it('clips hover backgrounds inside the outer rounded border', () => {
    const { container } = render(
      <ExecutionTraceCard
        title="子代理结果"
        badge="4 轮迭代"
        summary="分析完成"
        sections={[{ title: '生成文件', items: ['report.xlsx'] }]}
        expandLabel="查看执行轨迹"
        expandedContent={<div>details</div>}
      />,
    )

    expect(container.firstElementChild).toHaveClass('overflow-hidden')
  })


  it('removes the header bottom divider when a header-collapsible card is collapsed', () => {
    render(
      <ExecutionTraceCard
        title="工具执行轨迹"
        badge="已完成 2 步"
        headerCollapsible
      >
        <div>tool rows</div>
      </ExecutionTraceCard>,
    )

    const header = screen.getByRole('button', { name: /工具执行轨迹/ })
    expect(header).toHaveClass('border-b')

    fireEvent.click(header)

    expect(header).not.toHaveClass('border-b')
    expect(header).toHaveClass('rounded-lg')
  })

})
