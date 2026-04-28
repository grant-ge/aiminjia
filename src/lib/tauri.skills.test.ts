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
  it('listSkills 调用后端 list_skills', async () => {
    invokeMock.mockResolvedValueOnce([])

    await listSkills()

    expect(invokeMock).toHaveBeenCalledWith('list_skills')
  })

  it('getPluginInfo 调用后端 get_plugin_info', async () => {
    invokeMock.mockResolvedValueOnce({ tools: [], skills: [] })

    await getPluginInfo()

    expect(invokeMock).toHaveBeenCalledWith('get_plugin_info')
  })

  it('installCustomSkill 调用后端 install_custom_skill', async () => {
    invokeMock.mockResolvedValueOnce('installed')

    await installCustomSkill('/tmp/skill')

    expect(invokeMock).toHaveBeenCalledWith('install_custom_skill', { sourcePath: '/tmp/skill', force: false })
  })
})
