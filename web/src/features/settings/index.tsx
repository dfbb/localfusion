import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { LoggingForm } from './components/logging-form'

export function Settings() {
  return (
    <>
      <Header fixed>
        <h1 className="text-base font-medium">设置</h1>
      </Header>

      <Main className="flex flex-1 flex-col gap-4 sm:gap-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">系统设置</h2>
          <p className="text-muted-foreground">日志和服务器配置。</p>
        </div>
        <LoggingForm />
      </Main>
    </>
  )
}
