/**
 * @designSource design.pen#PqcAk > mascot+hello
 * @sizing mascot 48×48, title 30/700, gap 12
 */
interface HomeMascotHeroProps {
  mascotUrl: string
  title: string
}

export function HomeMascotHero({ mascotUrl, title }: HomeMascotHeroProps) {
  return (
    <div className="flex items-center gap-3">
      <div data-testid="home-mascot" className="h-12 w-12 overflow-hidden rounded-md">
        <img src={mascotUrl} alt="" className="w-full" />
      </div>
      <div className="text-3xl font-bold leading-9 text-foreground">
        {title}
      </div>
    </div>
  )
}
