import { createHashRouter } from 'react-router-dom'

import { AppLayout } from '../components/layout/AppLayout'

export const appRouter = createHashRouter([
  {
    path: '/',
    element: <AppLayout />
  }
])
