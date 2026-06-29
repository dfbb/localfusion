import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  type ColumnFiltersState,
  type SortingState,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  getFacetedRowModel,
  getFacetedUniqueValues,
  useReactTable,
} from '@tanstack/react-table'
import { ChevronLeft, ChevronRight, Search, X } from 'lucide-react'
import { api } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { type ModelRow, type Prices } from '../data/schema'
import { modelsColumns } from './models-columns'

const CONNECTORS = ['chat', 'anthropic', 'responses']

export function ModelsTable() {
  const { t } = useTranslation()
  const [sorting, setSorting] = useState<SortingState>([])
  const [columnFilters, setColumnFilters] = useState<ColumnFiltersState>([])

  const { data = [], isLoading } = useQuery<ModelRow[]>({
    queryKey: ['models'],
    queryFn: () => api.get('/models').then((r) => r.data),
  })

  const { data: prices = [] } = useQuery<Prices[]>({
    queryKey: ['prices'],
    queryFn: () => api.get('/stats/prices').then((r) => r.data),
  })
  const priceMap = new Map(prices.map((p) => [p.model_id, p]))

  const table = useReactTable({
    data,
    columns: modelsColumns,
    state: { sorting, columnFilters },
    onSortingChange: setSorting,
    onColumnFiltersChange: setColumnFilters,
    getCoreRowModel: getCoreRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    getFacetedRowModel: getFacetedRowModel(),
    getFacetedUniqueValues: getFacetedUniqueValues(),
    meta: { priceMap },
  })

  const idFilter = (table.getColumn('id')?.getFilterValue() as string) ?? ''
  const connectorFilter = (table.getColumn('connector')?.getFilterValue() as string[]) ?? []

  function toggleConnector(v: string) {
    const cur = connectorFilter
    const next = cur.includes(v) ? cur.filter((c) => c !== v) : [...cur, v]
    table.getColumn('connector')?.setFilterValue(next.length ? next : undefined)
  }

  return (
    <div className="flex flex-1 flex-col gap-4">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative">
          <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
          <Input
          placeholder={t('models.searchId')}
            value={idFilter}
            onChange={(e) => table.getColumn('id')?.setFilterValue(e.target.value || undefined)}
            className="pl-8 h-8 w-48"
          />
        </div>
        <div className="flex items-center gap-1">
          {CONNECTORS.map((c) => (
            <Badge
              key={c}
              variant={connectorFilter.includes(c) ? 'default' : 'outline'}
              className="cursor-pointer select-none"
              onClick={() => toggleConnector(c)}
            >
              {c}
            </Badge>
          ))}
          {connectorFilter.length > 0 && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2 text-xs"
              onClick={() => table.getColumn('connector')?.setFilterValue(undefined)}
            >
              <X className="h-3 w-3 mr-1" />
              {t('common.reset')}
            </Button>
          )}
        </div>
      </div>

      {/* Table */}
      <div className="overflow-hidden rounded-md border">
        <Table>
          <TableHeader>
            {table.getHeaderGroups().map((hg) => (
              <TableRow key={hg.id}>
                {hg.headers.map((header) => (
                  <TableHead key={header.id}>
                    {header.isPlaceholder
                      ? null
                      : flexRender(header.column.columnDef.header, header.getContext())}
                  </TableHead>
                ))}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {isLoading ? (
              <TableRow>
                <TableCell colSpan={modelsColumns.length} className="h-24 text-center text-muted-foreground">
                  {t('common.loading')}
                </TableCell>
              </TableRow>
            ) : table.getRowModel().rows.length ? (
              table.getRowModel().rows.map((row) => (
                <TableRow key={row.id}>
                  {row.getVisibleCells().map((cell) => (
                    <TableCell key={cell.id}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </TableCell>
                  ))}
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={modelsColumns.length} className="h-24 text-center text-muted-foreground">
                  {t('models.noModels')}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      {/* Pagination */}
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          {t('common.totalRows', { count: table.getFilteredRowModel().rows.length })}
        </p>
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
    </div>
  )
}
