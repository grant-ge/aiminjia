import { createContext, useContext } from 'react'

import type { ExpertTeam } from '@/features/expert-teams/teams'

const TeamVisualContext = createContext<ExpertTeam | null>(null)

export const TeamVisualProvider = TeamVisualContext.Provider

export function useTeamVisualContext(): ExpertTeam | null {
  return useContext(TeamVisualContext)
}
