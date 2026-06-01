import { create } from 'zustand'

import type { PreviewTarget } from '@/components/chat/generatedFileActions'

interface GeneratedFilePreviewState {
  target: PreviewTarget | null
  openPreview: (target: PreviewTarget) => void
  closePreview: () => void
  clearIfConversationChanged: (conversationId: string) => void
  reset: () => void
}

export const useGeneratedFilePreviewStore = create<GeneratedFilePreviewState>((set, get) => ({
  target: null,
  openPreview: (target) => set({ target }),
  closePreview: () => set({ target: null }),
  clearIfConversationChanged: (conversationId) => {
    const current = get().target
    if (current && current.conversationId !== conversationId) set({ target: null })
  },
  reset: () => set({ target: null }),
}))
