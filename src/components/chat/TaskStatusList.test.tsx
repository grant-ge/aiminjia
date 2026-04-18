import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { TaskStatusList } from './TaskStatusList'
import type { ConversationTaskState } from '@/stores/streamingStore'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { count?: number; defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}))

describe('TaskStatusList', () => {
  it('renders nothing when tasks array is empty', () => {
    const { container } = render(<TaskStatusList tasks={[]} />)
    expect(container.firstChild).toBeNull()
  })

  it('renders running task with spinner icon', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-abcd1234', status: 'running', runId: 'run-1' },
    ]

    render(<TaskStatusList tasks={tasks} />)

    expect(screen.getByRole('img', { name: /running/i })).toBeTruthy()
  })

  it('renders completed task with check icon', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-abcd1234', status: 'completed', runId: 'run-1' },
    ]

    render(<TaskStatusList tasks={tasks} />)

    expect(screen.getByRole('img', { name: /completed/i })).toBeTruthy()
  })

  it('renders failed task with red icon', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-abcd1234', status: 'failed', runId: 'run-1' },
    ]

    render(<TaskStatusList tasks={tasks} />)

    expect(screen.getByRole('img', { name: /failed/i })).toBeTruthy()
  })

  it('auto-expands when any task is running', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-1', status: 'completed', runId: 'run-1' },
      { taskId: 'task-2', status: 'running', runId: 'run-2' },
    ]

    render(<TaskStatusList tasks={tasks} />)

    expect(screen.getAllByRole('listitem')).toHaveLength(2)
  })

  it('is collapsed by default when no running tasks', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-1', status: 'completed', runId: 'run-1' },
      { taskId: 'task-2', status: 'completed', runId: 'run-2' },
    ]
    const { container } = render(<TaskStatusList tasks={tasks} />)
    const details = container.querySelector('details')

    expect(details?.hasAttribute('open')).toBe(false)
  })

  it('displays the last 8 characters of the task id', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-abcd1234', status: 'pending', runId: 'run-1' },
    ]

    render(<TaskStatusList tasks={tasks} />)

    expect(screen.getByText(/abcd1234/)).toBeTruthy()
  })
})
