/**
 * @designSource design.pen#PqcAk > mascot+hello
 * @sizing mascot 40×40, title 24/700, gap 12
 */
interface HomeMascotHeroProps {
  mascotUrl: string
  title: string
}

export function HomeMascotHero({ mascotUrl, title }: HomeMascotHeroProps) {
  return (
    <div className="flex items-center gap-3">
      <div data-testid="home-mascot" className="h-10 w-10 overflow-hidden rounded-md border border-border bg-card">
        <img src={mascotUrl} alt="" className="w-full" />
      </div>
      <div className="text-2xl font-bold leading-8 text-foreground">
        {title}
      </div>
    </div>
  )
}
