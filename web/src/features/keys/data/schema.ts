export type KeyRow = {
  id: string
  label: string
  enabled: boolean
  acl_all: boolean
  created_at: number // unix timestamp seconds
}

export type KeyCreateResult = {
  id: string
  key: string
}
