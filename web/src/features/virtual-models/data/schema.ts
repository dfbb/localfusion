import { z } from 'zod'

export const virtualModelSchema = z.object({
  name: z.string().min(1, '名称不能为空'),
  strategy: z.string().min(1, '请选择策略'),
  members: z.array(z.string()).min(1, '至少需要一个成员'),
  params: z.record(z.string(), z.unknown()).optional(),
})

export type VirtualModelForm = z.infer<typeof virtualModelSchema>

export type VirtualModelRow = {
  name: string
  strategy: string
  members: string[]
  params: Record<string, unknown>
}

export type StrategyParamSchema = {
  type: string
  description?: string
  enum?: string[]
  default?: unknown
  minimum?: number
  maximum?: number
  'x-ref'?: string
}

export type StrategyRow = {
  name: string
  description?: string
  params_schema?: {
    type: string
    properties?: Record<string, StrategyParamSchema>
    required?: string[]
  }
}
