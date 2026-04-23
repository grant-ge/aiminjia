/**
 * @designSource design.pen#PqcAk > mascot+hello+subHello
 * @sizing mascot 64×64 r-full, title 30/700, subtitle 14 muted, gap 16
 */
interface HomeMascotHeroProps {
  mascotUrl: string
  title: string
  subtitle: string
}

export function HomeMascotHero({ mascotUrl, title, subtitle }: HomeMascotHeroProps) {
  return (
    <div className="flex flex-col items-center gap-4">
      <div
        data-testid="home-mascot"
        className="h-16 w-16 overflow-hidden rounded-full"
      >
        <img src={mascotUrl} alt="" className="h-full w-full object-cover" />
      </div>
      <div className="text-[30px] font-bold leading-tight text-foreground">
        {title}
      </div>
      <div className="max-w-[760px] text-center text-sm text-muted-foreground">
        {subtitle}
      </div>
    </div>
  )
}
