import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { TrendingUp, Hash, ArrowDownUp, DollarSign, Layers } from 'lucide-react'
import { api } from '@/lib/api'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'

interface Summary {
  requests: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  cost: number
}

function fmt(n: number) {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(n)
}

export function SummaryCards() {
  const { t } = useTranslation()
  const { data, isLoading } = useQuery<Summary>({
    queryKey: ['usage-summary'],
    queryFn: () => api.get('/stats/usage/summary').then((r) => r.data),
  })

  const CARDS = [
    { key: 'requests' as const, label: t('dashboard.cardRequests'), icon: Hash, format: fmt },
    { key: 'input_tokens' as const, label: t('dashboard.cardInputTokens'), icon: ArrowDownUp, format: fmt },
    { key: 'output_tokens' as const, label: t('dashboard.cardOutputTokens'), icon: TrendingUp, format: fmt },
    { key: 'total_tokens' as const, label: t('dashboard.cardTotalTokens'), icon: Layers, format: fmt },
    {
      key: 'cost' as const, label: t('dashboard.cardTotalCost'), icon: DollarSign,
      format: (v: number) => '$' + v.toFixed(4),
    },
  ]

  return (
    <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-5">
      {CARDS.map(({ key, label, icon: Icon, format }) => (
        <Card key={key}>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">{label}</CardTitle>
            <Icon className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-8 w-24" />
            ) : (
              <p className="text-2xl font-bold">{format(data?.[key] ?? 0)}</p>
            )}
          </CardContent>
        </Card>
      ))}
    </div>
  )
}
