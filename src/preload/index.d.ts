import type { PlannerApi } from './index'

declare global {
  interface Window {
    planner: PlannerApi
  }
}

export {}
