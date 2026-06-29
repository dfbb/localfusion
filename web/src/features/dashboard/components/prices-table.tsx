import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { formatDistanceToNow } from 'date-fns'
import { zhCN } from 'date-fns/locale'
import { api } from '@/lib/api'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

interface PriceRow {
  model_id: string
  price_in: number  // $/M tokens
  price_out: number // $/M tokens
  updated_at: number // Unix seconds
}

export function PricesTable() {
  const { t, i18n } = useTranslation()
  const { data: rows = [], isLoading } = useQuery<PriceRow[]>({
    queryKey: ['prices'],
    queryFn: () => api.get('/stats/prices').then((r) => r.data),
  })

  if (isLoading) return <Skeleton className="h-40 w-full" />

  const now = Date.now()
  const sevenDaysMs = 7 * 24 * 60 * 60 * 1000
  const dateFnsLocale = i18n.language === 'zh' ? zhCN : undefined

  return (
    <div className="overflow-hidden rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t('dashboard.colModel')}</TableHead>
            <TableHead>{t('dashboard.colPriceIn')}</TableHead>
            <TableHead>{t('dashboard.colPriceOut')}</TableHead>
            <TableHead>{t('dashboard.colUpdatedAt')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.length ? (
            rows.map((row) => {
              const stale = now - row.updated_at * 1000 > sevenDaysMs
              return (
                <TableRow key={row.model_id} className={cn(stale && 'bg-yellow-50 dark:bg-yellow-950/20')}>
                  <TableCell className="font-mono text-xs">{row.model_id}</TableCell>
                  <TableCell>${row.price_in.toFixed(4)}</TableCell>
                  <TableCell>${row.price_out.toFixed(4)}</TableCell>
                  <TableCell className={cn('text-xs', stale && 'text-yellow-600 dark:text-yellow-400')}>
                    {formatDistanceToNow(new Date(row.updated_at * 1000), { addSuffix: true, locale: dateFnsLocale })}
                    {stale && ' ⚠️'}
                  </TableCell>
                </TableRow>
              )
            })
          ) : (
            <TableRow>
              <TableCell colSpan={4} className="h-20 text-center text-muted-foreground text-sm">
                {t('dashboard.noPriceData')}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
