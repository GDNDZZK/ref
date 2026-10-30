import { createPinia } from 'pinia'
import { createApp } from 'vue'
import App from './App.vue'
import { applyTheme } from './composables/useTheme.ts'
import { createAppI18n } from './i18n/index.ts'
import { boot } from './stores/boot.ts'
import { useConnectionStore } from './stores/connection.ts'
import './theme/generated/theme.css'
import './styles/base.css'

applyTheme()

const pinia = createPinia()
createApp(App).use(pinia).use(createAppI18n()).mount('#app')

void boot().then((error) => {
  if (error) useConnectionStore().fatal = error.messageKey
})
