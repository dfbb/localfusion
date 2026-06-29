import { useTranslation } from 'react-i18next'
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
  const { t } = useTranslation()
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
      toast.success(t('settings.savedWithRestart'))
    },
    onError: () => {
      toast.error(t('common.saveFailed'))
    },
  })

  const logToStdout = watch('log_to_stdout')
  const logLevel = watch('log_level')

  if (isLoading) {
    return <div className="text-muted-foreground text-sm">{t('common.loading')}</div>
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('settings.loggingTitle')}</CardTitle>
        <CardDescription>{t('settings.loggingDescription')}</CardDescription>
      </CardHeader>
      <CardContent>
        <form onSubmit={handleSubmit((v) => save.mutate(v))} className="space-y-6">
          <div className="space-y-2">
            <label className="text-sm font-medium">{t('settings.logLevel')}</label>
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
            <label className="text-sm font-medium">{t('settings.logFilePath')}</label>
            <Input
              {...register('log_file')}
              placeholder={t('settings.logFilePlaceholder')}
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
              {t('settings.logToStdout')}
            </label>
          </div>

          <Button type="submit" disabled={save.isPending}>
            {save.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </form>
      </CardContent>
    </Card>
  )
}
