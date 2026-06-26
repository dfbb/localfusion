import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
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
import { type VirtualModelRow, type StrategyRow } from '../data/schema'
import { virtualModelsColumns } from './virtual-models-columns'

export function VirtualModelsTable() {
  const [sorting, setSorting] = useState<SortingState>([])
  const [columnFilters, setColumnFilters] = useState<ColumnFiltersState>([])

  const { data = [], isLoading } = useQuery<VirtualModelRow[]>({
    queryKey: ['vmodels'],
    queryFn: () => api.get('/virtual-models').then((r) => r.data),
  })

  const { data: strategies = [] } = useQuery<StrategyRow[]>({
    queryKey: ['strategies'],
    queryFn: () => api.get('/strategies').then((r) => r.data),
  })

  const strategyNames = strategies.map((s) => s.name)

  const table = useReactTable({
    data,
    columns: virtualModelsColumns,
    state: { sorting, columnFilters },
    onSortingChange: setSorting,
    onColumnFiltersChange: setColumnFilters,
    getCoreRowModel: getCoreRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    getFacetedRowModel: getFacetedRowModel(),
    getFacetedUniqueValues: getFacetedUniqueValues(),
  })

  const nameFilter = (table.getColumn('name')?.getFilterValue() as string) ?? ''
  const strategyFilter = (table.getColumn('strategy')?.getFilterValue() as string[]) ?? []

  function toggleStrategy(v: string) {
    const cur = strategyFilter
    const next = cur.includes(v) ? cur.filter((c) => c !== v) : [...cur, v]
    table.getColumn('strategy')?.setFilterValue(next.length ? next : undefined)
  }

  return (
    <div className="flex flex-1 flex-col gap-4">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative">
          <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
          <Input
            placeholder="搜索名称..."
            value={nameFilter}
            onChange={(e) => table.getColumn('name')?.setFilterValue(e.target.value || undefined)}
            className="pl-8 h-8 w-48"
          />
        </div>
        <div className="flex items-center gap-1 flex-wrap">
          {strategyNames.map((s) => (
            <Badge
              key={s}
              variant={strategyFilter.includes(s) ? 'default' : 'outline'}
              className="cursor-pointer select-none"
              onClick={() => toggleStrategy(s)}
            >
              {s}
            </Badge>
          ))}
          {strategyFilter.length > 0 && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2 text-xs"
              onClick={() => table.getColumn('strategy')?.setFilterValue(undefined)}
            >
              <X className="h-3 w-3 mr-1" />
              重置
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
                <TableCell colSpan={virtualModelsColumns.length} className="h-24 text-center text-muted-foreground">
                  加载中...
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
                <TableCell colSpan={virtualModelsColumns.length} className="h-24 text-center text-muted-foreground">
                  暂无虚拟模型
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      {/* Pagination */}
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          共 {table.getFilteredRowModel().rows.length} 条
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
