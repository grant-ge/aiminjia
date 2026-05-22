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
 * - 占位策略:`value` 为空(begin_registration 还未返回) → 显示居中 spinner +
 *   "二维码加载中…"文案。旧版用一个 7×7 黑白格子假装是二维码,容易被用户
 *   误以为"扫码失败 / bug",换成显式 loading 状态。
 * - `loading=true` 时在已有 QR 上叠 backdrop + spinner(用于刷新过期的 QR),
 *   不卸载 <img> 避免闪烁。
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
        // value 还没拿到 → 显式 loading 占位。bg-white 保持二维码区视觉一致,
        // 不闪到 bg-background 让弹窗看起来跳。
        <div
          data-testid="qr-placeholder-loading"
          aria-label={alt}
          aria-busy="true"
          role="status"
          className="flex h-full w-full flex-col items-center justify-center gap-3 rounded bg-white"
        >
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
          <p className="text-xs font-medium text-zinc-500">二维码加载中…</p>
        </div>
      )}
      {loading && qrDataUrl && (
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
