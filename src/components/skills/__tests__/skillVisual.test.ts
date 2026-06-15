import { describe, expect, it } from 'vitest'

import { getSkillAvatarSrc } from '../skillVisual'

describe('skillVisual', () => {
  it('does not force supplied jpgs onto skills that should use text fallback', () => {
    expect(getSkillAvatarSrc('biz-proposal')).toBeNull()
    expect(getSkillAvatarSrc('comp-analysis-v2')).toBeNull()
    expect(getSkillAvatarSrc('perf-system-design')).toBeNull()
    expect(getSkillAvatarSrc('labor-compliance')).toBeNull()
    expect(getSkillAvatarSrc('pa-maturity')).toBeNull()
    expect(getSkillAvatarSrc('resume-screening')).toBeNull()
  })

  it('uses every local jpg from the supplied icon folder at least once', () => {
    const mappedIds = [
      'bid-writing',
      'biz-writing',
      'budget-analysis',
      'competitive-intelligence',
      'engagement-survey',
      'html-ppt',
      'okr-coach',
      'ops-analysis',
      'org-diagnosis',
      'policy-compliance-audit',
      'recruitment-funnel',
    ]
    expect(new Set(mappedIds.map((id) => getSkillAvatarSrc(id)))).toEqual(
      new Set([
        '/skill-avatars/bid-writing.jpg',
        '/skill-avatars/biz-writing.jpg',
        '/skill-avatars/competitive-intelligence.jpg',
        '/skill-avatars/engagement-survey.jpg',
        '/skill-avatars/finance-yuan.jpg',
        '/skill-avatars/html-ppt.jpg',
        '/skill-avatars/okr-coach.jpg',
        '/skill-avatars/ops-analysis.jpg',
        '/skill-avatars/org-diagnosis.jpg',
        '/skill-avatars/policy-compliance-audit.jpg',
        '/skill-avatars/recruitment-funnel.jpg',
      ]),
    )
  })
})
