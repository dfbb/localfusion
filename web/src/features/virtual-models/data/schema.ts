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
