import { useState } from 'react'
import { subHours } from 'date-fns'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { RangePicker, type DateRange } from './components/range-picker'
import { SummaryCards } from './components/summary-cards'
import { UsageChart } from './components/usage-chart'
import { UsageRanking } from './components/usage-ranking'
import { LatencyChart } from './components/latency-chart'
import { PricesTable } from './components/prices-table'
import { RequestsTable } from './components/requests-table'

function defaultRange(): DateRange {
  const to = new Date()
  const from = subHours(to, 24)
  return { from: from.toISOString(), to: to.toISOString(), granularity: 'hour' }
}

export function Dashboard() {
  const [range, setRange] = useState<DateRange>(defaultRange)

  return (
    <div className="flex flex-col gap-6">
      {/* Time Range */}
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">监控面板</h2>
          <p className="text-muted-foreground text-sm">查看用量、性能与价格数据。</p>
        </div>
        <RangePicker value={range} onChange={setRange} />
      </div>

      {/* Summary Cards */}
      <SummaryCards />

      {/* Usage Chart + Ranking Tabs */}
      <Tabs defaultValue="chart">
        <TabsList>
          <TabsTrigger value="chart">用量趋势</TabsTrigger>
          <TabsTrigger value="ranking">模型排行</TabsTrigger>
        </TabsList>
        <TabsContent value="chart" className="mt-4">
          <UsageChart range={range} />
        </TabsContent>
        <TabsContent value="ranking" className="mt-4">
          <UsageRanking range={range} />
        </TabsContent>
      </Tabs>

      {/* Latency */}
      <div>
        <h3 className="mb-3 text-base font-semibold">吞吐延迟</h3>
        <LatencyChart />
      </div>

      {/* Prices */}
      <div>
        <h3 className="mb-3 text-base font-semibold">价格表</h3>
        <PricesTable />
      </div>

      {/* Requests */}
      <div>
        <h3 className="mb-3 text-base font-semibold">请求明细</h3>
        <RequestsTable />
      </div>
    </div>
  )
}
