/**
 * Persona store — manages user-defined roles and active persona.
 */
import { create } from 'zustand'
import type { Persona, PersonaSummary } from '@/lib/tauri'
import * as tauri from '@/lib/tauri'

interface PersonaStore {
  personas: PersonaSummary[]
  activePersona: Persona | null
  loading: boolean

  // Actions
  reload: () => Promise<void>
  setActive: (id: string) => Promise<void>
  save: (persona: Persona) => Promise<void>
  delete: (id: string) => Promise<void>
  exportPersona: (id: string) => Promise<string>
  importPersona: (json: string) => Promise<string>
}

export const usePersonaStore = create<PersonaStore>((set, get) => ({
  personas: [],
  activePersona: null,
  loading: false,

  reload: async () => {
    set({ loading: true })
    try {
      const [personas, activePersona] = await Promise.all([
        tauri.listPersonas(),
        tauri.getActivePersona(),
      ])
      set({ personas, activePersona, loading: false })
    } catch (error) {
      console.error('Failed to reload personas:', error)
      set({ loading: false })
    }
  },

  setActive: async (id: string) => {
    await tauri.setActivePersona(id)
    const activePersona = await tauri.getPersona(id)
    set({ activePersona })
  },

  save: async (persona: Persona) => {
    await tauri.savePersona(persona)
    await get().reload()
  },

  delete: async (id: string) => {
    await tauri.deletePersona(id)
    await get().reload()
  },

  exportPersona: async (id: string) => {
    return await tauri.exportPersonas(id)
  },

  importPersona: async (json: string) => {
    const newId = await tauri.importPersonas(json)
    await get().reload()
    return newId
  },
}))
