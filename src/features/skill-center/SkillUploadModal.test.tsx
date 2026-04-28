import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const openMock = vi.hoisted(() => vi.fn())
const askMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openMock,
  ask: askMock,
}))

import { SkillUploadModal } from './SkillUploadModal'
import { useSkillStore, SkillAlreadyExistsError } from '@/stores/skillStore'
import { useNotificationStore } from '@/stores/notificationStore'

describe('SkillUploadModal', () => {
  beforeEach(() => {
    openMock.mockReset()
    askMock.mockReset()
    useSkillStore.setState({
      skills: [],
      recommendedIds: [],
      isLoading: false,
      upload: vi.fn().mockResolvedValue(undefined),
    })
    useNotificationStore.setState({ notifications: [] })
  })

  it('提示只支持包含 SKILL.md 的技能目录', () => {
    render(<SkillUploadModal open onOpenChange={vi.fn()} />)

    expect(screen.getByText(/选择一个包含/)).toHaveTextContent('选择一个包含 SKILL.md 的本地技能目录，安装后会自动刷新技能中心。')
    expect(screen.queryByText(/plugin\.toml/)).not.toBeInTheDocument()
  })

  it('选择本地技能目录后调用 upload 并关闭弹窗', async () => {
    const onOpenChange = vi.fn()
    const upload = vi.fn().mockResolvedValue(undefined)
    openMock.mockResolvedValue('/tmp/custom-skill')
    useSkillStore.setState({ upload })

    render(<SkillUploadModal open onOpenChange={onOpenChange} />)
    fireEvent.click(screen.getByRole('button', { name: '选择技能目录' }))

    await waitFor(() => expect(upload).toHaveBeenCalledWith('/tmp/custom-skill'))
    expect(onOpenChange).toHaveBeenCalledWith(false)

    const notification = useNotificationStore.getState().notifications.at(-1)
    expect(notification?.level).toBe('success')
    expect(notification?.title).toBe('技能上传成功')

  })

  it('上传失败时显示错误 toast 并保留弹窗', async () => {
    const onOpenChange = vi.fn()
    const upload = vi.fn().mockRejectedValue(new Error('manifest missing'))
    openMock.mockResolvedValue('/tmp/broken-skill')
    useSkillStore.setState({ upload })

    render(<SkillUploadModal open onOpenChange={onOpenChange} />)
    fireEvent.click(screen.getByRole('button', { name: '选择技能目录' }))

    await waitFor(() => expect(upload).toHaveBeenCalledWith('/tmp/broken-skill'))
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
    const notification = useNotificationStore.getState().notifications.at(-1)
    expect(notification?.level).toBe('error')
    expect(notification?.title).toBe('技能上传失败')
    expect(notification?.message).toContain('manifest missing')
  })

  it('重复技能时弹出确认对话框，用户确认后覆盖安装并关闭弹窗', async () => {
    const onOpenChange = vi.fn()
    const upload = vi.fn()
      .mockRejectedValueOnce(new SkillAlreadyExistsError('dup-skill'))
      .mockResolvedValueOnce(undefined)
    openMock.mockResolvedValue('/tmp/dup-skill')
    askMock.mockResolvedValue(true)
    useSkillStore.setState({ upload })

    render(<SkillUploadModal open onOpenChange={onOpenChange} />)
    fireEvent.click(screen.getByRole('button', { name: '选择技能目录' }))

    await waitFor(() => expect(askMock).toHaveBeenCalledWith('技能 "dup-skill" 已存在，是否覆盖？', { title: 'AI小家', kind: 'warning' }))
    await waitFor(() => expect(upload).toHaveBeenCalledWith('/tmp/dup-skill', true))
    expect(onOpenChange).toHaveBeenCalledWith(false)
    const notification = useNotificationStore.getState().notifications.at(-1)
    expect(notification?.level).toBe('success')
  })

  it('重复技能时弹出确认对话框，用户取消后不提示错误也不关闭弹窗', async () => {
    const onOpenChange = vi.fn()
    const upload = vi.fn().mockRejectedValueOnce(new SkillAlreadyExistsError('dup-skill'))
    openMock.mockResolvedValue('/tmp/dup-skill')
    askMock.mockResolvedValue(false)
    useSkillStore.setState({ upload })

    render(<SkillUploadModal open onOpenChange={onOpenChange} />)
    fireEvent.click(screen.getByRole('button', { name: '选择技能目录' }))

    await waitFor(() => expect(askMock).toHaveBeenCalled())
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
    expect(useNotificationStore.getState().notifications).toHaveLength(0)
  })
})
