import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useForm } from 'react-hook-form'
import { toast } from 'sonner'
import { api } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
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
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'

type LoggingConfig = {
  log_level: string
  log_file: string
  log_to_stdout: boolean
}

export function LoggingForm() {
  const qc = useQueryClient()

  const { data, isLoading } = useQuery<LoggingConfig>({
    queryKey: ['logging'],
    queryFn: () => api.get('/settings/logging').then((r) => r.data),
  })

  const { register, handleSubmit, watch, setValue } = useForm<LoggingConfig>({
    values: data ?? { log_level: 'info', log_file: '', log_to_stdout: true },
  })

  const save = useMutation({
    mutationFn: (v: LoggingConfig) => api.put('/settings/logging', v),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['logging'] })
      toast.success('已保存（文件/控制台改动需重启）')
    },
    onError: () => {
      toast.error('保存失败')
    },
  })

  const logToStdout = watch('log_to_stdout')
  const logLevel = watch('log_level')

  if (isLoading) {
    return <div className="text-muted-foreground text-sm">加载中...</div>
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>日志配置</CardTitle>
        <CardDescription>配置日志级别和输出目标。修改文件/控制台选项需重启服务生效。</CardDescription>
      </CardHeader>
      <CardContent>
        <form onSubmit={handleSubmit((v) => save.mutate(v))} className="space-y-6">
          <div className="space-y-2">
            <label className="text-sm font-medium">日志级别</label>
            <Select
              value={logLevel}
              onValueChange={(v) => setValue('log_level', v)}
            >
              <SelectTrigger className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="debug">debug</SelectItem>
                <SelectItem value="info">info</SelectItem>
                <SelectItem value="error">error</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">日志文件路径</label>
            <Input
              {...register('log_file')}
              placeholder="留空表示不写文件（如 /var/log/localfusion.log）"
              className="max-w-md"
            />
          </div>

          <div className="flex items-center gap-3">
            <Switch
              id="log_to_stdout"
              checked={logToStdout}
              onCheckedChange={(v) => setValue('log_to_stdout', v)}
            />
            <label htmlFor="log_to_stdout" className="text-sm font-medium cursor-pointer">
              输出到控制台（stdout）
            </label>
          </div>

          <Button type="submit" disabled={save.isPending}>
            {save.isPending ? '保存中...' : '保存'}
          </Button>
        </form>
      </CardContent>
    </Card>
  )
}
