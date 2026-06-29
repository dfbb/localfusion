import { useState } from 'react'
import { useTranslation } from 'react-i18next'
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
  return {
    from: Math.floor(from.getTime() / 1000),
    to: Math.floor(to.getTime() / 1000),
    granularity: 'hour',
  }
}

export function Dashboard() {
  const { t } = useTranslation()
  const [range, setRange] = useState<DateRange>(defaultRange)

  return (
    <div className="flex flex-col gap-6">
      {/* Time Range */}
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">{t('nav.dashboardPage')}</h2>
          <p className="text-muted-foreground text-sm">{t('dashboard.subtitle')}</p>
        </div>
        <RangePicker value={range} onChange={setRange} />
      </div>

      {/* Summary Cards */}
      <SummaryCards />

      {/* Usage Chart + Ranking Tabs */}
      <Tabs defaultValue="chart">
        <TabsList>
          <TabsTrigger value="chart">{t('dashboard.tabUsage')}</TabsTrigger>
          <TabsTrigger value="ranking">{t('dashboard.tabRanking')}</TabsTrigger>
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
        <h3 className="mb-3 text-base font-semibold">{t('dashboard.sectionLatency')}</h3>
        <LatencyChart />
      </div>

      {/* Prices */}
      <div>
        <h3 className="mb-3 text-base font-semibold">{t('dashboard.sectionPrices')}</h3>
        <PricesTable />
      </div>

      {/* Requests */}
      <div>
        <h3 className="mb-3 text-base font-semibold">{t('dashboard.sectionRequests')}</h3>
        <RequestsTable />
      </div>
    </div>
  )
}
