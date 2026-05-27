import { describe, expect, it, vi } from 'vitest'

const invokeMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

import { getPluginInfo, installCustomSkill, listSkills } from './tauri'

describe('skill tauri ipc wrappers', () => {
  it('listSkills passes the active language to list_skills', async () => {
    invokeMock.mockResolvedValueOnce([])

    await listSkills('en-US')

    expect(invokeMock).toHaveBeenCalledWith('list_skills', { language: 'en-US' })
  })

  it('getPluginInfo passes the active language to get_plugin_info', async () => {
    invokeMock.mockResolvedValueOnce({ tools: [], skills: [] })

    await getPluginInfo('en-US')

    expect(invokeMock).toHaveBeenCalledWith('get_plugin_info', { language: 'en-US' })
  })

  it('installCustomSkill 调用后端 install_custom_skill', async () => {
    invokeMock.mockResolvedValueOnce('installed')

    await installCustomSkill('/tmp/skill')

    expect(invokeMock).toHaveBeenCalledWith('install_custom_skill', { sourcePath: '/tmp/skill', force: false })
  })
})
