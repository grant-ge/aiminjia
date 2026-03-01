/**
 * Plugin store — registered tools and skills.
 */
import { create } from 'zustand'
import type { ToolInfo, SkillInfo } from '@/lib/tauri'

interface PluginState {
  tools: ToolInfo[]
  skills: SkillInfo[]
  isLoaded: boolean

  setTools: (tools: ToolInfo[]) => void
  setSkills: (skills: SkillInfo[]) => void
  setAll: (tools: ToolInfo[], skills: SkillInfo[]) => void
  markLoaded: () => void
}

export const usePluginStore = create<PluginState>((set) => ({
  tools: [],
  skills: [],
  isLoaded: false,

  setTools: (tools) => set({ tools }),
  setSkills: (skills) => set({ skills }),
  setAll: (tools, skills) => set({ tools, skills, isLoaded: true }),
  markLoaded: () => set({ isLoaded: true }),
}))
