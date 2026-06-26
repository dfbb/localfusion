import { useState } from 'react'
import { useQuery, useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'
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
        toast.error(`运行失败: ${data.error}`)
      } else {
        toast.success('运行完成')
      }
    },
    onError: () => {
      toast.error('请求失败')
    },
  })

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!virtualName) {
      toast.error('请选择虚拟模型')
      return
    }
    run.mutate({ virtual_name: virtualName, prompt })
  }

  return (
    <>
      <Header fixed>
        <h1 className="text-base font-medium">调试台</h1>
      </Header>

      <Main className="flex flex-1 flex-col gap-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Playground</h2>
          <p className="text-muted-foreground">向虚拟模型发送测试请求，查看策略编排细节。</p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">发送请求</CardTitle>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleSubmit} className="space-y-4">
              <div className="space-y-2">
                <label className="text-sm font-medium">虚拟模型</label>
                <Select value={virtualName} onValueChange={setVirtualName}>
                  <SelectTrigger className="w-64">
                    <SelectValue placeholder="选择虚拟模型..." />
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
                <label className="text-sm font-medium">Prompt</label>
                <textarea
                  value={prompt}
                  onChange={(e) => setPrompt(e.target.value)}
                  placeholder="输入测试 prompt..."
                  rows={4}
                  className="flex w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 max-w-2xl font-mono resize-y"
                />
              </div>

              <Button type="submit" disabled={run.isPending}>
                {run.isPending ? '运行中...' : '发送'}
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
                  <CardTitle className="text-base text-destructive">错误</CardTitle>
                </CardHeader>
                <CardContent>
                  <pre className="text-sm text-destructive whitespace-pre-wrap">{result.error}</pre>
                </CardContent>
              </Card>
            ) : (
              <>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">最终回答</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <pre className="text-sm whitespace-pre-wrap font-mono">{result.final || '(无内容)'}</pre>
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
                <CardTitle className="text-base">调用明细</CardTitle>
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
