import { useEffect, useState } from 'react'

import { expertTeamTemplateCatalog } from '@/lib/tauri'
import { BUILTIN_EXPERT_TEAMS, snapshotToExpertTeam, type ExpertTeam } from './teams'

export type ExpertTeamCatalogSource = 'remote' | 'bootstrap'

let catalogById = new Map(BUILTIN_EXPERT_TEAMS.map((team) => [team.id, team]))

export function seedExpertTeamCatalog(teams: ExpertTeam[]) {
  catalogById = new Map(teams.map((team) => [team.id, team]))
}

export function getCachedExpertTeam(teamId: string): ExpertTeam | undefined {
  return catalogById.get(teamId)
}

export function useExpertTeamCatalog() {
  const [teams, setTeams] = useState<ExpertTeam[]>(BUILTIN_EXPERT_TEAMS)
  const [source, setSource] = useState<ExpertTeamCatalogSource>('bootstrap')
  const [isLoading, setIsLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const snapshots = await expertTeamTemplateCatalog()
        if (!cancelled && snapshots.length > 0) {
          const remoteTeams = snapshots.map(snapshotToExpertTeam)
          seedExpertTeamCatalog(remoteTeams)
          setTeams(remoteTeams)
          setSource('remote')
        }
      } catch (err) {
        console.warn('[expert-teams] catalog load failed, using bootstrap:', err)
      } finally {
        if (!cancelled) setIsLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  return { teams, source, isLoading }
}
