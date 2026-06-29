import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from 'recharts'
import { api } from '@/lib/api'
import { Skeleton } from '@/components/ui/skeleton'

interface LatencyRow {
  model_id: string
  avg_throughput: number // tokens/sec
  sample_count: number
}

export function LatencyChart() {
  const { t } = useTranslation()
  const { data: rows = [], isLoading } = useQuery<LatencyRow[]>({
    queryKey: ['latency'],
    queryFn: () => api.get('/stats/latency').then((r) => r.data),
  })

  if (isLoading) return <Skeleton className="h-48 w-full" />

  if (!rows.length) {
    return (
      <div className="flex h-32 items-center justify-center rounded-md border text-sm text-muted-foreground">
        {t('dashboard.noLatencyData')}
      </div>
    )
  }

  const chartData = rows.map((r) => ({
    model: r.model_id.length > 20 ? r.model_id.slice(0, 20) + '…' : r.model_id,
    throughput: Math.round(r.avg_throughput),
    samples: r.sample_count,
  }))

  return (
    <ResponsiveContainer width="100%" height={240}>
      <BarChart data={chartData} margin={{ top: 8, right: 16, left: 0, bottom: 40 }}>
        <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
        <XAxis dataKey="model" tick={{ fontSize: 10 }} angle={-20} textAnchor="end" />
        <YAxis tick={{ fontSize: 11 }} label={{ value: 'tok/s', angle: -90, position: 'insideLeft', style: { fontSize: 10 } }} />
        <Tooltip
          formatter={(v, name) =>
            name === 'throughput'
              ? [`${v} tok/s`, t('dashboard.throughputLabel')]
              : [`${v}`, name]
          }
        />
        <Bar dataKey="throughput" name="throughput" fill="hsl(221 83% 53%)" radius={[3, 3, 0, 0]} />
      </BarChart>
    </ResponsiveContainer>
  )
}
