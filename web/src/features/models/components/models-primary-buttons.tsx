import { Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useModels } from './models-provider'

export function ModelsPrimaryButtons() {
  const { setOpen } = useModels()
  return (
    <Button onClick={() => setOpen('add')}>
      <Plus className="mr-2 h-4 w-4" />
      新建模型
    </Button>
  )
}
