/**
 * @designSource design.pen#TSZyx
 * @sizing logo 56×56 r-28; brand 22/600; gap 10
 */
interface LoginLogoStackProps {
  logoUrl: string
  brandName: string
}

export function LoginLogoStack({ logoUrl, brandName }: LoginLogoStackProps) {
  return (
    <div className="flex flex-col items-center gap-2.5">
      <div className="h-14 w-14 overflow-hidden rounded-md">
        <img src={logoUrl} alt="" className="h-full w-full object-cover" />
      </div>
      <div className="text-[1.375rem] font-semibold text-foreground">{brandName}</div>
    </div>
  )
}
