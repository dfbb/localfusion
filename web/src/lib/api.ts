import axios from 'axios'
import { useAuth } from '@/stores/auth-store'

export const api = axios.create({ baseURL: '/admin/api' })

api.interceptors.request.use((cfg) => {
  const t = useAuth.getState().token
  if (t) cfg.headers.Authorization = `Bearer ${t}`
  return cfg
})

api.interceptors.response.use(
  (r) => r,
  (err) => {
    if (err.response?.status === 401) {
      useAuth.getState().setToken(null)
      window.location.href = '/sign-in'
    }
    return Promise.reject(err)
  }
)
