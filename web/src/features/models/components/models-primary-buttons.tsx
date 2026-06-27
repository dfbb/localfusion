import { Loader2, Plus, Zap } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useModels } from './models-provider'

export function ModelsPrimaryButtons() {
  const { setOpen, testing, runTestAll } = useModels()
  return (
    <div className="flex items-center gap-2">
      <Button onClick={() => setOpen('add')}>
        <Plus className="mr-2 h-4 w-4" />
        新建模型
      </Button>
      <Button variant="outline" onClick={runTestAll} disabled={testing}>
        {testing ? (
          <>
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            Testing…
          </>
        ) : (
          <>
            <Zap className="mr-2 h-4 w-4" />
            Test All
          </>
        )}
      </Button>
    </div>
  )
}
