import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const openMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openMock,
}))

import { SkillUploadModal } from './SkillUploadModal'
import { useSkillStore } from '@/stores/skillStore'
import { useNotificationStore } from '@/stores/notificationStore'

describe('SkillUploadModal', () => {
  beforeEach(() => {
    openMock.mockReset()
    useSkillStore.setState({
      skills: [],
      recommendedIds: [],
      isLoading: false,
      upload: vi.fn().mockResolvedValue(undefined),
    })
    useNotificationStore.setState({ notifications: [] })
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
})
