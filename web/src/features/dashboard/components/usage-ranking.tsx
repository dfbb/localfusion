import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  type SortingState,
  useReactTable,
} from '@tanstack/react-table'
import { api } from '@/lib/api'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Skeleton } from '@/components/ui/skeleton'
import type { DateRange } from './range-picker'

interface UsageRow {
  hour_ts: number
  name: string
  scope: string
  requests: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  cost: number
  errors: number
}

interface AggRow {
  name: string
  requests: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  cost: number
  errors: number
}

function useAggData(scope: 'real' | 'virtual', range: DateRange) {
  const { data: rows = [], isLoading } = useQuery<UsageRow[]>({
    queryKey: ['usage-ranking', scope, range.from, range.to],
    queryFn: () =>
      api
        .get('/stats/usage', { params: { scope, from: range.from, to: range.to } })
        .then((r) => r.data),
  })

  const agg = useMemo<AggRow[]>(() => {
    const map = new Map<string, AggRow>()
    for (const row of rows) {
      const existing = map.get(row.name)
      if (existing) {
        existing.requests += row.requests
        existing.input_tokens += row.input_tokens
        existing.output_tokens += row.output_tokens
        existing.total_tokens += row.total_tokens
        existing.cost += row.cost
        existing.errors += row.errors
      } else {
        map.set(row.name, { ...row })
      }
    }
    return Array.from(map.values()).sort((a, b) => b.total_tokens - a.total_tokens)
  }, [rows])

  return { agg, isLoading }
}

function RankingTable({ scope, range }: { scope: 'real' | 'virtual'; range: DateRange }) {
  const { t } = useTranslation()
  const [sorting, setSorting] = useState<SortingState>([{ id: 'total_tokens', desc: true }])
  const { agg, isLoading } = useAggData(scope, range)

  const columns = useMemo<ColumnDef<AggRow>[]>(() => [
    { accessorKey: 'name', header: t('dashboard.colModel'), cell: ({ getValue }) => <span className="font-mono text-xs">{getValue<string>()}</span> },
    { accessorKey: 'requests', header: scope === 'real' ? t('dashboard.colRealRequests') : t('dashboard.colRequests') },
    { accessorKey: 'input_tokens', header: t('dashboard.colInputTokens'), cell: ({ getValue }) => getValue<number>().toLocaleString() },
    { accessorKey: 'output_tokens', header: t('dashboard.colOutputTokens'), cell: ({ getValue }) => getValue<number>().toLocaleString() },
    { accessorKey: 'total_tokens', header: t('dashboard.colTotalTokens'), cell: ({ getValue }) => getValue<number>().toLocaleString() },
    { accessorKey: 'cost', header: t('dashboard.colCost'), cell: ({ getValue }) => '$' + getValue<number>().toFixed(4) },
    { accessorKey: 'errors', header: t('dashboard.colErrors') },
  ], [t, scope])

  const table = useReactTable({
    data: agg,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  })

  if (isLoading) return <Skeleton className="h-40 w-full" />

  return (
    <div className="overflow-hidden rounded-md border">
      <Table>
        <TableHeader>
          {table.getHeaderGroups().map((hg) => (
            <TableRow key={hg.id}>
              {hg.headers.map((header) => (
                <TableHead
                  key={header.id}
                  className="cursor-pointer select-none whitespace-nowrap text-xs"
                  onClick={header.column.getToggleSortingHandler()}
                >
                  {flexRender(header.column.columnDef.header, header.getContext())}
                  {header.column.getIsSorted() === 'asc' ? ' ↑' : header.column.getIsSorted() === 'desc' ? ' ↓' : ''}
                </TableHead>
              ))}
            </TableRow>
          ))}
        </TableHeader>
        <TableBody>
          {table.getRowModel().rows.length ? (
            table.getRowModel().rows.map((row) => (
              <TableRow key={row.id}>
                {row.getVisibleCells().map((cell) => (
                  <TableCell key={cell.id} className="text-xs">
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </TableCell>
                ))}
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell colSpan={columns.length} className="h-20 text-center text-muted-foreground text-sm">
                {t('common.noData')}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}

interface Props {
  range: DateRange
}

export function UsageRanking({ range }: Props) {
  const { t } = useTranslation()
  return (
    <Tabs defaultValue="real">
      <TabsList>
        <TabsTrigger value="real">{t('dashboard.tabRealModels')}</TabsTrigger>
        <TabsTrigger value="virtual">{t('dashboard.tabVirtualModels')}</TabsTrigger>
      </TabsList>
      <TabsContent value="real" className="mt-4">
        <RankingTable scope="real" range={range} />
      </TabsContent>
      <TabsContent value="virtual" className="mt-4">
        <RankingTable scope="virtual" range={range} />
      </TabsContent>
    </Tabs>
  )
}
