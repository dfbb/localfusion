import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from 'recharts'
import { format } from 'date-fns'
import { api } from '@/lib/api'
import { Skeleton } from '@/components/ui/skeleton'
import type { DateRange, Granularity } from './range-picker'

interface UsageRow {
  hour_ts: number // unix seconds
  name: string
  scope: string
  requests: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  cost: number
  errors: number
}

interface ChartPoint {
  bucket: string
  total_tokens: number
  cost: number
}

function bucketKey(ts: number, granularity: Granularity): number {
  if (granularity === 'hour') return ts
  if (granularity === 'day') return Math.floor(ts / 86400) * 86400
  return Math.floor(ts / (86400 * 7)) * (86400 * 7)
}

function bucketLabel(ts: number, granularity: Granularity): string {
  const d = new Date(ts * 1000)
  if (granularity === 'hour') return format(d, 'MM-dd HH:mm')
  if (granularity === 'day') return format(d, 'MM-dd')
  return format(d, 'MM-dd')
}

interface Props {
  range: DateRange
}

export function UsageChart({ range }: Props) {
  const { data: rows = [], isLoading } = useQuery<UsageRow[]>({
    queryKey: ['usage', range.from, range.to],
    queryFn: () =>
      api
        .get('/stats/usage', { params: { scope: 'total', from: range.from, to: range.to } })
        .then((r) => r.data),
  })

  const chartData = useMemo<ChartPoint[]>(() => {
    const map = new Map<number, ChartPoint>()
    for (const row of rows) {
      const bk = bucketKey(row.hour_ts, range.granularity)
      const existing = map.get(bk)
      if (existing) {
        existing.total_tokens += row.total_tokens
        existing.cost += row.cost
      } else {
        map.set(bk, {
          bucket: bucketLabel(bk, range.granularity),
          total_tokens: row.total_tokens,
          cost: row.cost,
        })
      }
    }
    return Array.from(map.entries())
      .sort((a, b) => a[0] - b[0])
      .map((e) => e[1])
  }, [rows, range.granularity])

  if (isLoading) return <Skeleton className="h-64 w-full" />

  return (
    <ResponsiveContainer width="100%" height={280}>
      <LineChart data={chartData} margin={{ top: 8, right: 24, left: 0, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
        <XAxis dataKey="bucket" tick={{ fontSize: 11 }} />
        <YAxis yAxisId="tokens" tick={{ fontSize: 11 }} tickFormatter={(v) => (v / 1000).toFixed(0) + 'K'} />
        <YAxis yAxisId="cost" orientation="right" tick={{ fontSize: 11 }} tickFormatter={(v) => '$' + v.toFixed(3)} />
        <Tooltip
          formatter={(value, name) =>
            name === 'total_tokens'
              ? [(value as number).toLocaleString() + ' tokens', 'Token']
              : ['$' + (value as number).toFixed(4), '费用']
          }
        />
        <Legend />
        <Line
          yAxisId="tokens"
          type="monotone"
          dataKey="total_tokens"
          name="total_tokens"
          stroke="hsl(221 83% 53%)"
          dot={false}
          strokeWidth={2}
        />
        <Line
          yAxisId="cost"
          type="monotone"
          dataKey="cost"
          name="cost"
          stroke="hsl(142 76% 36%)"
          dot={false}
          strokeWidth={2}
        />
      </LineChart>
    </ResponsiveContainer>
  )
}
