import { useState } from 'react'
import { useQuery, useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'
import { useTranslation } from 'react-i18next'
import { api } from '@/lib/api'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { type VirtualModelRow } from '@/features/virtual-models/data/schema'
import { TraceView } from './components/trace-view'
import { CallsTable, type CallRecord } from './components/calls-table'

type PlaygroundResult = {
  final?: string
  strategy?: string
  status?: string
  calls?: CallRecord[]
  detail?: Record<string, unknown>
  error?: string
}

export function Playground() {
  const { t } = useTranslation()
  const [virtualName, setVirtualName] = useState('')
  const [prompt, setPrompt] = useState('')
  const [result, setResult] = useState<PlaygroundResult | null>(null)

  const { data: vmodels = [] } = useQuery<VirtualModelRow[]>({
    queryKey: ['vmodels'],
    queryFn: () => api.get('/virtual-models').then((r) => r.data),
  })

  const run = useMutation({
    mutationFn: (v: { virtual_name: string; prompt: string }) =>
      api.post('/playground', v).then((r) => r.data),
    onSuccess: (data: PlaygroundResult) => {
      setResult(data)
      if (data.error) {
        toast.error(t('playground.runFailed', { error: data.error }))
      } else {
        toast.success(t('playground.runSuccess'))
      }
    },
    onError: () => {
      toast.error(t('playground.requestFailed'))
    },
  })

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!virtualName) {
      toast.error(t('playground.selectModelRequired'))
      return
    }
    run.mutate({ virtual_name: virtualName, prompt })
  }

  return (
    <>
      <Header fixed>
        <h1 className="text-base font-medium">{t('nav.playground')}</h1>
      </Header>

      <Main className="flex flex-1 flex-col gap-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">{t('nav.playground')}</h2>
          <p className="text-muted-foreground">{t('playground.subtitle')}</p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('playground.sendRequest')}</CardTitle>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleSubmit} className="space-y-4">
              <div className="space-y-2">
                <label className="text-sm font-medium">{t('playground.virtualModelLabel')}</label>
                <Select value={virtualName} onValueChange={setVirtualName}>
                  <SelectTrigger className="w-64">
                    <SelectValue placeholder={t('playground.selectVirtualModel')} />
                  </SelectTrigger>
                  <SelectContent>
                    {vmodels.map((vm) => (
                      <SelectItem key={vm.name} value={vm.name}>
                        <span>{vm.name}</span>
                        <span className="text-muted-foreground ml-2 text-xs">{vm.strategy}</span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="space-y-2">
                <label className="text-sm font-medium">{t('playground.promptLabel')}</label>
                <textarea
                  value={prompt}
                  onChange={(e) => setPrompt(e.target.value)}
                  placeholder={t('playground.promptPlaceholder')}
                  rows={4}
                  className="flex w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 max-w-2xl font-mono resize-y"
                />
              </div>

              <Button type="submit" disabled={run.isPending}>
                {run.isPending ? t('playground.running') : t('playground.send')}
              </Button>
            </form>
          </CardContent>
        </Card>

        {result && (
          <div className="space-y-4">
            <Separator />

            {result.error ? (
              <Card>
                <CardHeader>
                  <CardTitle className="text-base text-destructive">{t('playground.errorTitle')}</CardTitle>
                </CardHeader>
                <CardContent>
                  <pre className="text-sm text-destructive whitespace-pre-wrap">{result.error}</pre>
                </CardContent>
              </Card>
            ) : (
              <>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">{t('playground.finalAnswer')}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <pre className="text-sm whitespace-pre-wrap font-mono">{result.final || t('playground.noContent')}</pre>
                  </CardContent>
                </Card>

                {result.detail && result.strategy && (
                  <TraceView
                    strategy={result.strategy}
                    detail={result.detail}
                  />
                )}
              </>
            )}

            <Card>
              <CardHeader>
                <CardTitle className="text-base">{t('playground.callsTitle')}</CardTitle>
              </CardHeader>
              <CardContent>
                <CallsTable calls={result.calls ?? []} />
              </CardContent>
            </Card>
          </div>
        )}
      </Main>
    </>
  )
}
