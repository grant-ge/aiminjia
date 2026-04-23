import { useState } from 'react'

import { LoginCard } from '@/components/auth/LoginCard'
import { LoginFooter } from '@/components/auth/LoginFooter'
import { LoginLogoStack } from '@/components/auth/LoginLogoStack'
import { LoginOptionsRow } from '@/components/auth/LoginOptionsRow'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'

export function LoginPage() {
  const login = useAuthStore((s) => s.login)
  const isAuthPending = useAuthStore((s) => s.isAuthPending)
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      setError('')
      await login(username, password)
    } catch (err) {
      setPassword('')
      setError(err instanceof Error ? err.message : '登录失败，请重试')
    }
  }

  return (
    <div className="flex min-h-screen w-full flex-col items-center justify-center gap-6 bg-background px-6">
      <LoginLogoStack logoUrl={logoUrl} brandName={productName} />
      <LoginCard>
        <div className="flex flex-col gap-1.5">
          <div className="text-[20px] font-semibold text-foreground">登录到 {productName}</div>
          <div className="text-[13px] text-muted-foreground">使用企业账号继续</div>
        </div>
        <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="username">账号</Label>
            <Input
              id="username"
              placeholder="请输入企业账号"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="password">密码</Label>
            <Input
              id="password"
              type="password"
              placeholder="请输入密码"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          <LoginOptionsRow
            rememberSlot={
              <label className="flex items-center gap-2 text-[13px] text-foreground">
                <input type="checkbox" defaultChecked className="rounded" />
                记住我
              </label>
            }
            onForget={() => {}}
          />
          {error ? (
            <div className="text-[13px] text-destructive">{error}</div>
          ) : null}
          <Button
            type="submit"
            disabled={isAuthPending}
            className="w-full rounded-full py-3 text-[15px] font-semibold"
          >
            登录
          </Button>
          <div className="text-center text-[12px] text-muted-foreground">
            登录即代表同意《服务条款》与《隐私政策》
          </div>
        </form>
      </LoginCard>
      <LoginFooter text="AI 小家 v0.9.30 · © 仁励家网络科技(杭州)有限公司" />
    </div>
  )
}
