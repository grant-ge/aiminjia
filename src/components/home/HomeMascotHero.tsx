/**
 * @designSource design.pen#PqcAk > mascot+hello
 * @sizing mascot 64×64, title 30/700, gap 16
 */
interface HomeMascotHeroProps {
  mascotUrl: string
  title: string
}

export function HomeMascotHero({ mascotUrl, title }: HomeMascotHeroProps) {
  return (
    <div className="flex items-center gap-4">
      <div data-testid="home-mascot" className="h-12 w-12 rounded-md overflow-hidden">
        <img src={mascotUrl} alt="" className="w-full" />
      </div>
      <div className="text-3xl font-bold leading-tight text-foreground">
        {title}
      </div>
    </div>
  )
}
