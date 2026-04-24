import { create } from 'zustand'
import type { InteractionRequiredPayload } from '@/lib/tauri'

interface InteractionState {
  pendingInteractions: InteractionRequiredPayload[]
  addInteraction: (payload: InteractionRequiredPayload) => void
  removeInteraction: (interactionId: string) => void
  clearForConversation: (conversationId: string) => void
}

export const useInteractionStore = create<InteractionState>((set) => ({
  pendingInteractions: [],

  addInteraction(payload) {
    set((state) => ({
      pendingInteractions: [
        ...state.pendingInteractions.filter((item) => item.interactionId !== payload.interactionId),
        payload,
      ],
    }))
  },

  removeInteraction(interactionId) {
    set((state) => ({
      pendingInteractions: state.pendingInteractions.filter(
        (interaction) => interaction.interactionId !== interactionId,
      ),
    }))
  },

  clearForConversation(conversationId) {
    set((state) => ({
      pendingInteractions: state.pendingInteractions.filter(
        (interaction) => interaction.conversationId !== conversationId,
      ),
    }))
  },
}))
