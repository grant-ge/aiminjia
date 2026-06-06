import {
  BadgeDollarSign,
  BarChart3,
  Compass,
  Megaphone,
  MessagesSquare,
  RotateCcw,
  Scale,
  SearchCheck,
  UsersRound,
  type LucideIcon,
} from 'lucide-react'

import type { ExpertTeamId } from './teams'

export const TEAM_LOGOS: Record<string, { icon: LucideIcon; className: string }> = {
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

export function getExpertTeamLogo(teamId: ExpertTeamId) {
  return TEAM_LOGOS[teamId] ?? {
    icon: UsersRound,
    className: 'bg-indigo-50 text-indigo-600',
  }
}
