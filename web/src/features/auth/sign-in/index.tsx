import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from '@tanstack/react-router'
import axios from 'axios'
import { useAuth } from '@/stores/auth-store'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Card } from '@/components/ui/card'

export function SignIn() {
  const { t } = useTranslation()
  const [token, setToken] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const setAuth = useAuth((s) => s.setToken)
  const nav = useNavigate()

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    setError('')
    try {
      await axios.get('/admin/api/health', {
        headers: { Authorization: `Bearer ${token}` },
      })
      setAuth(token)
      nav({ to: '/' })
    } catch {
      setError(t('auth.invalidToken'))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="flex min-h-svh items-center justify-center bg-muted/50">
      <Card className="w-80 p-6">
        <form onSubmit={submit} className="space-y-4">
          <h1 className="text-xl font-bold">{t('auth.title')}</h1>
          <Input
            type="password"
            placeholder={t('auth.tokenPlaceholder')}
            value={token}
            onChange={(e) => setToken(e.target.value)}
            autoFocus
          />
          {error && <p className="text-sm text-destructive">{error}</p>}
          <Button type="submit" className="w-full" disabled={loading}>
            {loading ? t('auth.verifying') : t('auth.signIn')}
          </Button>
        </form>
      </Card>
    </div>
  )
}
