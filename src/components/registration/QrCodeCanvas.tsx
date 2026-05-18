import { useEffect, useState } from 'react'
import QRCode from 'qrcode'
import { Loader2 } from 'lucide-react'

interface QrCodeCanvasProps {
  /** Any string payload (URL, token, etc.) to be encoded into a QR image. */
  value: string
  /** Show a spinner overlay; used while begin_registration is in-flight. */
  loading: boolean
  /** Accessible label for the generated <img>. */
  alt?: string
}

/**
 * Renders a QR code from a string payload using the `qrcode` library.
 *
 * - White background is **fixed** (not theme-driven): WeChat / DingTalk scanner
 *   apps fail to decode against dark backgrounds.
 * - When `value` is empty, shows a structural 7x7 grid placeholder so the layout
 *   doesn't jump while begin_registration is still in flight.
 * - When `loading=true`, overlays a backdrop + spinner without unmounting the
 *   QR image (avoids flash on re-render).
 */
export function QrCodeCanvas({ value, loading, alt = '注册二维码' }: QrCodeCanvasProps) {
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null)

  useEffect(() => {
    if (!value) {
      setQrDataUrl(null)
      return
    }
    let cancelled = false
    QRCode.toDataURL(value, {
      errorCorrectionLevel: 'M',
      margin: 1,
      width: 224,
      color: { dark: '#111111', light: '#ffffff' },
    })
      .then((url) => {
        if (!cancelled) setQrDataUrl(url)
      })
      .catch(() => {
        if (!cancelled) setQrDataUrl(null)
      })
    return () => {
      cancelled = true
    }
  }, [value])

  return (
    <div className="relative flex h-60 w-60 items-center justify-center rounded-3xl border border-border bg-white p-4">
      {qrDataUrl ? (
        <img src={qrDataUrl} alt={alt} className="h-full w-full" />
      ) : (
        // Structural placeholder: black dots on white grid, keeps slot height stable
        <div
          aria-label={alt}
          data-testid="qr-placeholder-grid"
          className="grid h-full w-full grid-cols-7 grid-rows-7 gap-1 rounded bg-white p-2"
        >
          {Array.from({ length: 49 }).map((_, index) => (
            <span
              key={index}
              className={`rounded-[2px] ${[0, 1, 2, 7, 14, 42, 43, 44, 48, 34, 24, 18, 12, 31, 39, 5, 10, 29, 36, 46].includes(index) ? 'bg-black' : 'bg-zinc-100'}`}
            />
          ))}
        </div>
      )}
      {loading && (
        <div
          data-testid="qr-spinner-overlay"
          className="absolute inset-4 flex items-center justify-center rounded-xl bg-background/75 backdrop-blur-[1px]"
        >
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
        </div>
      )}
    </div>
  )
}
