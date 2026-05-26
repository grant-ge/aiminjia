import { useEffect, useState } from 'react'

import { expertTeamTemplateCatalog } from '@/lib/tauri'
import { BUILTIN_EXPERT_TEAMS, snapshotToExpertTeam, type ExpertTeam } from './teams'

export type ExpertTeamCatalogSource = 'remote' | 'bootstrap'

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
          setTeams(snapshots.map(snapshotToExpertTeam))
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
