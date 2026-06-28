import { useState, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { format } from 'date-fns'
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  type SortingState,
  useReactTable,
} from '@tanstack/react-table'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { api } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Skeleton } from '@/components/ui/skeleton'

interface RequestRow {
  id: number
  created_at: number // Unix seconds
  virtual_name: string | null
  strategy: string | null
  status: string | null
  total_tokens: number | null
  cost: number | null
}

const STATUS_COLORS: Record<string, string> = {
  ok: 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400',
  error: 'bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400',
  degraded: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400',
}

// Status values from backend — not translated
const STATUS_VALUES = ['ok', 'degraded', 'error']

export function RequestsTable() {
  const { t } = useTranslation()
  const [sorting, setSorting] = useState<SortingState>([{ id: 'created_at', desc: true }])
  const [statusFilter, setStatusFilter] = useState<string | null>(null)

  const { data: rows = [], isLoading } = useQuery<RequestRow[]>({
    queryKey: ['requests'],
    queryFn: () => api.get('/stats/requests').then((r) => r.data),
  })

  const filteredData = statusFilter === null ? rows : rows.filter((r) => r.status === statusFilter)

  const columns = useMemo<ColumnDef<RequestRow>[]>(() => [
    {
      accessorKey: 'created_at',
      header: t('dashboard.colTime'),
      cell: ({ getValue }) => (
        <span className="text-xs text-muted-foreground whitespace-nowrap">
          {format(new Date(getValue<number>() * 1000), 'MM-dd HH:mm:ss')}
        </span>
      ),
    },
    {
      accessorKey: 'status',
      header: t('dashboard.colStatus'),
      cell: ({ getValue }) => {
        const s = getValue<string | null>() ?? '-'
        return (
          <span className={`inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium ${STATUS_COLORS[s] ?? 'bg-muted'}`}>
            {s}
          </span>
        )
      },
    },
    {
      accessorKey: 'virtual_name',
      header: t('dashboard.colVirtualModel'),
      cell: ({ getValue }) => <span className="font-mono text-xs">{getValue<string | null>() ?? '-'}</span>,
    },
    {
      accessorKey: 'strategy',
      header: t('dashboard.colStrategy'),
      cell: ({ getValue }) => <span className="font-mono text-xs">{getValue<string | null>() ?? '-'}</span>,
    },
    {
      accessorKey: 'total_tokens',
      header: t('dashboard.colTotalTokens'),
      cell: ({ getValue }) => (getValue<number | null>() ?? 0).toLocaleString(),
    },
    {
      accessorKey: 'cost',
      header: t('dashboard.colCost'),
      cell: ({ getValue }) => '$' + (getValue<number | null>() ?? 0).toFixed(5),
    },
  ], [t])

  const table = useReactTable({
    data: filteredData,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    initialState: { pagination: { pageSize: 20 } },
  })

  return (
    <div className="flex flex-col gap-3">
      {/* Status filter */}
      <div className="flex flex-wrap items-center gap-2">
        <Badge
          variant={statusFilter === null ? 'default' : 'outline'}
          className="cursor-pointer select-none"
          onClick={() => setStatusFilter(null)}
        >
          {t('dashboard.statusAll')}
        </Badge>
        {STATUS_VALUES.map((s) => (
          <Badge
            key={s}
            variant={statusFilter === s ? 'default' : 'outline'}
            className="cursor-pointer select-none"
            onClick={() => setStatusFilter(s)}
          >
            {s}
          </Badge>
        ))}
      </div>

      {isLoading ? (
        <Skeleton className="h-48 w-full" />
      ) : (
        <>
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
                      {t('dashboard.noRequests')}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>

          <div className="flex items-center justify-between">
            <p className="text-sm text-muted-foreground">{t('dashboard.totalCount', { count: filteredData.length })}</p>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => table.previousPage()}
                disabled={!table.getCanPreviousPage()}
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              <span className="text-sm">
                {table.getState().pagination.pageIndex + 1} / {Math.max(table.getPageCount(), 1)}
              </span>
              <Button
                variant="outline"
                size="sm"
                onClick={() => table.nextPage()}
                disabled={!table.getCanNextPage()}
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </>
      )}
    </div>
  )
}
