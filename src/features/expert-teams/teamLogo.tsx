import {
  BadgeDollarSign,
  BarChart3,
  Compass,
  Megaphone,
  MessagesSquare,
  RotateCcw,
  Scale,
  SearchCheck,
  type LucideIcon,
} from 'lucide-react'

import type { ExpertTeamId } from './teams'

type BuiltinExpertTeamId =
  | 'marketing'
  | 'operations'
  | 'strategy'
  | 'negotiation'
  | 'retrospective'
  | 'investment'
  | 'debate'
  | 'roundtable'

interface ExpertTeamLogo {
  icon: LucideIcon
  className: string
}

const DEFAULT_TEAM_LOGO: ExpertTeamLogo = {
  icon: SearchCheck,
  className: 'bg-slate-100 text-slate-600',
}

export const TEAM_LOGOS: Record<BuiltinExpertTeamId, ExpertTeamLogo> = {
  marketing: {
    icon: Megaphone,
    className: 'bg-rose-50 text-rose-600',
  },
  operations: {
    icon: BarChart3,
    className: 'bg-sky-50 text-sky-600',
  },
  strategy: {
    icon: Compass,
    className: 'bg-violet-50 text-violet-600',
  },
  negotiation: {
    icon: MessagesSquare,
    className: 'bg-amber-50 text-amber-700',
  },
  retrospective: {
    icon: RotateCcw,
    className: 'bg-emerald-50 text-emerald-600',
  },
  investment: {
    icon: BadgeDollarSign,
    className: 'bg-teal-50 text-teal-700',
  },
  debate: {
    icon: Scale,
    className: 'bg-orange-50 text-orange-700',
  },
  roundtable: {
    icon: SearchCheck,
    className: 'bg-slate-100 text-slate-600',
  },
}

export function getExpertTeamLogo(teamId: ExpertTeamId): ExpertTeamLogo {
  if (Object.prototype.hasOwnProperty.call(TEAM_LOGOS, teamId)) {
    return TEAM_LOGOS[teamId as BuiltinExpertTeamId]
  }
  return DEFAULT_TEAM_LOGO
}
