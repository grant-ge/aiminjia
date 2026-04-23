/**
 * @designSource design.pen#DWw8D metaRow
 * @sizing gap 48; label 13 muted; value 14 foreground
 */
interface SkillMetaItem {
  label: string
  value: string
}

interface SkillMetaRowProps {
  items: SkillMetaItem[]
}

export function SkillMetaRow({ items }: SkillMetaRowProps) {
  return (
    <div className="flex w-full flex-wrap items-start gap-12">
      {items.map((it) => (
        <div key={it.label} className="flex flex-col gap-1.5">
          <div className="text-[13px] text-muted-foreground">{it.label}</div>
          <div className="text-sm text-foreground">{it.value}</div>
        </div>
      ))}
    </div>
  )
}
