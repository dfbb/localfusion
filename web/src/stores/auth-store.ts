import { create } from 'zustand'

const KEY = 'lf_admin_token'

type AuthState = {
  token: string | null
  setToken: (t: string | null) => void
}

export const useAuth = create<AuthState>((set) => ({
  token: sessionStorage.getItem(KEY),
  setToken: (t) => {
    if (t) sessionStorage.setItem(KEY, t)
    else sessionStorage.removeItem(KEY)
    set({ token: t })
  },
}))
