// Must be first: installs Array.prototype.findLast etc. for Big Sur Safari 14.
import '@/legacy-polyfills'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@/i18n'
import '@/styles/globals.css'
import App from './App'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
